//! Highlights the other uses of the symbol at the caret, as VS Code's
//! `editor.occurrencesHighlight` does: the language server's document
//! highlights where it provides them, write uses stronger than reads, else
//! the word's whole-word occurrences.
//!
//! Every open editor has one [`CaretHighlights`] provider, installed once.
//! Only the active editor shows any: for a caret in (or a selection of) one
//! word, once it has rested [`REST`] and the server answered. Moving again
//! restarts the wait, and an answer for an old caret is dropped.

use std::{cell::RefCell, collections::HashMap, ops::Range, pin::Pin, rc::Rc, time::Duration};

use fe_buffer::{BufferSnapshot, Version};
use fe_editor::decorations::{
    Decoration, DecorationKind, DecorationProvider, ProviderId, Tint, overlaps,
};
use fe_editor::occurrences::OccurrenceHighlights;
use fe_gpui::EditorView;
use fe_lsp::{DocumentHighlight, HighlightKind};
use gpui::{App, Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long the caret rests before highlights are asked for: VS Code's
/// `editor.occurrencesHighlightDelay`.
const REST: Duration = Duration::from_millis(250);

/// Under the editor's own marks, as word occurrences are.
const PRIORITY: i32 = -50;

/// What a request answers: `None` for an error.
type Answer = Option<Option<Vec<DocumentHighlight>>>;

/// The server's highlights of one version of the buffer.
struct Served {
    version: Version,
    items: Vec<DocumentHighlight>,
}

/// What an editor shows: the server's highlights, or the word's
/// occurrences, never both.
#[derive(Default)]
struct CaretHighlights {
    served: Option<Served>,
    words: OccurrenceHighlights,
    /// The version the words were set at.
    words_version: Option<Version>,
}

impl CaretHighlights {
    /// Shows nothing; whether anything was.
    fn clear(&mut self, snapshot: &BufferSnapshot) -> bool {
        let had = self.served.take().is_some();
        self.words_version = None;
        self.words.set_target(snapshot, None) | had
    }

    /// Whether `word`, in `snapshot`, is inside a range shown.
    fn shows(&self, snapshot: &BufferSnapshot, word: &Range<usize>) -> bool {
        let shown = match &self.served {
            Some(served) => Some(served.version),
            None => self.words_version,
        };
        if shown != Some(snapshot.version()) {
            return false;
        }
        let mut out = Vec::new();
        self.decorations(snapshot, word.clone(), &mut out);
        out.iter()
            .any(|item| item.range.start <= word.start && word.end <= item.range.end)
    }
}

impl DecorationProvider for CaretHighlights {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let Some(served) = &self.served else {
            self.words.decorations(snapshot, range, out);
            return;
        };
        if served.version != snapshot.version() {
            return;
        }
        let len = snapshot.text().len();
        for item in &served.items {
            if item.range.end > len || !overlaps(&item.range, &range) {
                continue;
            }
            let tint = match item.kind {
                HighlightKind::Write => Tint::Accent,
                HighlightKind::Text | HighlightKind::Read => Tint::Muted,
            };
            out.push(
                Decoration::new(item.range.clone(), DecorationKind::Highlight, tint)
                    .with_priority(PRIORITY),
            );
        }
    }
}

/// Where an editor's caret asks for highlights: the buffer version and the
/// word, and the offset to ask at.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Wanted {
    version: Version,
    word: Range<usize>,
    /// Where the caret is: moving within the word waits again.
    at: usize,
}

struct Slot {
    editor: Entity<EditorView>,
    provider: Rc<RefCell<CaretHighlights>>,
    id: ProviderId,
    wanted: Option<Wanted>,
    /// The wait for the caret to rest, then the request.
    task: Option<Task<()>>,
}

/// Every open editor's highlights.
#[derive(Default)]
pub(super) struct Highlights {
    slots: HashMap<EntityId, Slot>,
}

/// What `editor`'s caret asks for, and the offset to ask at.
fn caret_of(editor: &Entity<EditorView>, cx: &App) -> Option<(Wanted, usize)> {
    let model = editor.read(cx).model();
    let word = model.occurrence_target()?;
    let at = model.primary_selection().range.start;
    let version = model.snapshot().version();
    Some((Wanted { version, word, at }, at))
}

/// What `fe_lsp` says of a buffer no server has open.
const NOT_OPEN: &str = "not open on a language server";

/// `result` as an [`Answer`]: a buffer no server serves, or a server
/// without the provider, falls back to the words; any other error shows
/// nothing.
pub(super) fn answer_of(
    result: Result<Option<Vec<DocumentHighlight>>, fe_lsp::LspError>,
) -> Answer {
    match result {
        Ok(found) => Some(found),
        Err(fe_lsp::LspError::Unavailable(why)) if why == NOT_OPEN => Some(None),
        Err(error) => {
            log::debug!("no document highlights: {error}");
            None
        }
    }
}

impl Workbench {
    /// Gives each open editor its provider, and brings what they show up
    /// to date with the caret: run by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_highlights(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let active = self.active_editor(cx).cloned();
        let slots = &mut self.lsp.highlights.slots;
        slots.retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
        for editor in &editors {
            if slots.contains_key(&editor.entity_id()) {
                continue;
            }
            // A split copies its original's providers: drop those, to have
            // only its own.
            let others: Vec<ProviderId> = slots.values().map(|slot| slot.id).collect();
            let provider = Rc::new(RefCell::new(CaretHighlights::default()));
            let registered = provider.clone();
            let id = editor.update(cx, |view, _| {
                for other in others {
                    view.model_mut().remove_decoration_provider(other);
                }
                view.model_mut().add_decoration_provider(registered)
            });
            slots.insert(
                editor.entity_id(),
                Slot {
                    editor: editor.clone(),
                    provider,
                    id,
                    wanted: None,
                    task: None,
                },
            );
        }
        for editor in &editors {
            let now = if active.as_ref() == Some(editor) {
                caret_of(editor, cx)
            } else {
                None
            };
            let Some(slot) = self.lsp.highlights.slots.get_mut(&editor.entity_id()) else {
                continue;
            };
            let target = now.as_ref().map(|(wanted, _)| wanted.clone());
            if slot.wanted == target {
                continue;
            }
            slot.wanted = target.clone();
            slot.task = None;
            let changed = {
                let model = editor.read(cx).model();
                let snapshot = model.snapshot();
                let mut provider = slot.provider.borrow_mut();
                match &target {
                    Some(wanted) if provider.shows(snapshot, &wanted.word) => false,
                    _ => provider.clear(snapshot),
                }
            };
            if changed {
                editor.update(cx, |_, cx| cx.notify());
            }
            let Some((wanted, at)) = now else {
                continue;
            };
            let id = editor.entity_id();
            let task = cx.spawn(async move |workbench, cx| {
                cx.background_executor().timer(REST).await;
                let Ok(reply) = workbench.update(cx, |workbench, cx| {
                    workbench.ask_highlights(id, &wanted, at, cx)
                }) else {
                    return;
                };
                let answer = reply.await;
                workbench
                    .update(cx, |workbench, cx| {
                        workbench.highlights_landed(id, &wanted, answer, cx)
                    })
                    .ok();
            });
            if let Some(slot) = self.lsp.highlights.slots.get_mut(&id) {
                slot.task = Some(task);
            }
        }
    }

    /// The request for `wanted` of editor `id`, sent now: nothing without a
    /// manager.
    fn ask_highlights(
        &self,
        id: EntityId,
        wanted: &Wanted,
        at: usize,
        cx: &App,
    ) -> Pin<Box<dyn std::future::Future<Output = Answer>>> {
        let (Some(manager), Some(slot)) = (&self.lsp.manager, self.lsp.highlights.slots.get(&id))
        else {
            return Box::pin(async { Some(None) });
        };
        let buffer_id = slot.editor.read(cx).model().buffer().id();
        let reply = super::lock(manager).document_highlights(buffer_id, wanted.version, at);
        Box::pin(async move { answer_of(reply.await) })
    }

    /// Shows `answer`, the reply to the request for `wanted` of editor
    /// `id`, if the editor is still active with that caret and version.
    fn highlights_landed(
        &mut self,
        id: EntityId,
        wanted: &Wanted,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.lsp.highlights.slots.get(&id) else {
            return;
        };
        let editor = slot.editor.clone();
        let current = (self.active_editor(cx) == Some(&editor))
            .then(|| caret_of(&editor, cx))
            .flatten()
            .map(|(caret, _)| caret);
        if current.as_ref() != Some(wanted) {
            return;
        }
        {
            let model = editor.read(cx).model();
            let snapshot = model.snapshot();
            let mut provider = slot.provider.borrow_mut();
            provider.clear(snapshot);
            match answer {
                Some(Some(items)) => {
                    provider.served = Some(Served {
                        version: wanted.version,
                        items,
                    });
                }
                Some(None) => {
                    provider
                        .words
                        .set_target(snapshot, Some(wanted.word.clone()));
                    provider.words_version = Some(wanted.version);
                }
                None => {}
            }
        }
        editor.update(cx, |_, cx| cx.notify());
    }
}
