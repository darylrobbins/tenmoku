//! The Outline panel and the breadcrumb symbols from the language server's
//! document symbols, as VS Code's outline does, with the tree-sitter outline
//! as the fallback.
//!
//! The active editor asks when it's opened and, after an edit, once the
//! buffer has rested [`REST`]. An answer for an old version is dropped, and
//! a request in flight is cancelled when a newer one is made. A non-empty
//! answer becomes an [`Outline`] handed to the panel
//! ([`OutlinePanel::set_served`]); no provider, an empty reply, an error or
//! a stopped server hand it `None`, and the panel shows tree-sitter's.

use std::{collections::HashMap, time::Duration};

use fe_buffer::{BufferSnapshot, Version};
use fe_gpui::EditorView;
use fe_lsp::DocumentSymbol;
use fe_syntax::{Outline, OutlineItem};
use gpui::{Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long the buffer rests before symbols are asked for again: VS Code's
/// outline delay.
const REST: Duration = Duration::from_millis(500);

/// The manager's generation, the server a buffer is on, and the run of the
/// server and open of the document the symbols belong to: crash recovery
/// and a reopened document keep the server but not these.
type Session = (
    u64,
    fe_lsp::ServerId,
    fe_lsp::SessionId,
    fe_lsp::IncarnationId,
);

/// What a request answers.
type Answer = Result<Option<Vec<DocumentSymbol>>, fe_lsp::LspError>;

struct Slot {
    /// The manager and server the buffer was synced to when asked: another
    /// is a new session, whose answers aren't the old one's.
    session: Option<Session>,
    editor: Entity<EditorView>,
    /// The version asked for, or being.
    asked: Option<Version>,
    /// The version first asked for, on open: asked at once, unlike the
    /// versions after an edit, answered or not.
    opened: Option<Version>,
    /// The version last answered, whatever the answer.
    answered: Option<Version>,
    task: Option<Task<()>>,
}

/// Every open editor's request.
#[derive(Default)]
pub(super) struct Symbols {
    slots: HashMap<EntityId, Slot>,
}

/// The outline of `symbols`, which are of `snapshot`'s text.
fn outline_of(symbols: Vec<DocumentSymbol>, snapshot: &BufferSnapshot) -> Outline {
    let text = snapshot.text();
    let len = snapshot.len();
    let items = symbols
        .into_iter()
        .map(|symbol| {
            let clamp = |range: std::ops::Range<usize>| range.start.min(len)..range.end.min(len);
            let name_range = clamp(symbol.selection_range);
            OutlineItem {
                name: symbol.name,
                detail: symbol.detail,
                kind: symbol.kind,
                range: clamp(symbol.range),
                row: text.offset_to_point(name_range.start).row,
                name_range,
                depth: symbol.depth,
                parent: symbol.parent,
            }
        })
        .collect();
    Outline {
        items,
        truncated: false,
        version: snapshot.version(),
    }
}

impl Slot {
    fn new(editor: Entity<EditorView>) -> Self {
        Self {
            editor,
            session: None,
            asked: None,
            opened: None,
            answered: None,
            task: None,
        }
    }
}

impl Workbench {
    /// The session `editor`'s buffer is open in on its server, if it is.
    fn session_of(&self, editor: &Entity<EditorView>, cx: &gpui::App) -> Option<Session> {
        let manager = self.lsp.manager.as_ref()?;
        let buffer = editor.read(cx).model().buffer().id();
        let manager = super::lock(manager);
        let server = manager.server_of(buffer)?;
        let (session, incarnation) = manager.active_identity(buffer, server)?;
        Some((self.lsp.generation, server, session, incarnation))
    }

    /// Asks for the active editor's symbols when its version has none
    /// asked or answered, and hands the panel each answer: run by
    /// [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_symbols(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let active = self.active_editor(cx).cloned();
        let outline = self.dock().read(cx).outline().clone();
        let gone: Vec<EntityId> = (self.lsp.symbols.slots.keys())
            .filter(|id| !editors.iter().any(|editor| editor.entity_id() == **id))
            .copied()
            .collect();
        for id in gone {
            self.lsp.symbols.slots.remove(&id);
            outline.update(cx, |panel, cx| panel.set_served(id, None, cx));
        }
        for editor in &editors {
            let id = editor.entity_id();
            let version = editor.read(cx).model().snapshot().version();
            let is_active = active.as_ref() == Some(editor);
            let session = self.session_of(editor, cx);
            let slot = self
                .lsp
                .symbols
                .slots
                .entry(id)
                .or_insert_with(|| Slot::new(editor.clone()));
            if slot.session != session {
                // The server stopped, restarted or was replaced, or the
                // manager was: what it said, and was asked, no longer holds.
                *slot = Slot {
                    session,
                    ..Slot::new(editor.clone())
                };
                outline.update(cx, |panel, cx| panel.set_served(id, None, cx));
            }
            if session.is_none() {
                // Nothing to ask yet; the tree-sitter outline stands.
                continue;
            }
            if !is_active {
                // Cancelled; what it showed stays for when it's active again.
                slot.asked = None;
                slot.task = None;
                continue;
            }
            if slot.asked == Some(version) || slot.answered == Some(version) {
                continue;
            }
            // On open, ask now; after an edit, once it has rested.
            let opened = *slot.opened.get_or_insert(version);
            let wait = if opened == version {
                Duration::ZERO
            } else {
                REST
            };
            slot.asked = Some(version);
            slot.task = Some(cx.spawn(async move |workbench, cx| {
                if !wait.is_zero() {
                    cx.background_executor().timer(wait).await;
                }
                let Ok(reply) =
                    workbench.update(cx, |workbench, cx| workbench.ask_symbols(id, version, cx))
                else {
                    return;
                };
                let answer = reply.await;
                workbench
                    .update(cx, |workbench, cx| {
                        workbench.symbols_landed(id, version, answer, cx)
                    })
                    .ok();
            }));
        }
    }

    /// The request for editor `id`'s symbols at `version`, sent now: `Ok(None)`
    /// without a manager or a provider.
    fn ask_symbols(
        &self,
        id: EntityId,
        version: Version,
        cx: &gpui::App,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Answer>>> {
        let (Some(manager), Some(slot)) = (&self.lsp.manager, self.lsp.symbols.slots.get(&id))
        else {
            return Box::pin(async { Ok(None) });
        };
        let buffer_id = slot.editor.read(cx).model().buffer().id();
        Box::pin(super::lock(manager).document_symbols(buffer_id, version))
    }

    /// Hands the panel `answer`, the reply to the request for editor `id`
    /// at `version`, if the editor is still at that version. A server not
    /// ready (not started or open yet) is asked again at the next frame.
    fn symbols_landed(
        &mut self,
        id: EntityId,
        version: Version,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.lsp.symbols.slots.get_mut(&id) else {
            return;
        };
        let editor = slot.editor.clone();
        let session = self.session_of(&editor, cx);
        let Some(slot) = self.lsp.symbols.slots.get_mut(&id) else {
            return;
        };
        if slot.session.is_none()
            || slot.session != session
            || slot.asked != Some(version)
            || editor.read(cx).model().snapshot().version() != version
        {
            return;
        }
        let (served, retry) = match answer {
            Ok(Some(symbols)) if !symbols.is_empty() => (
                Some(outline_of(symbols, editor.read(cx).model().snapshot())),
                false,
            ),
            Ok(_) => (None, false),
            Err(
                fe_lsp::LspError::Unavailable(_)
                | fe_lsp::LspError::Retry
                | fe_lsp::LspError::Cancelled
                | fe_lsp::LspError::ServerDown,
            ) => (None, true),
            Err(error) => {
                log::debug!("no document symbols: {error}");
                (None, false)
            }
        };
        if retry {
            slot.asked = None;
        } else {
            slot.answered = Some(version);
        }
        let outline = self.dock().read(cx).outline().clone();
        outline.update(cx, |panel, cx| panel.set_served(id, served, cx));
    }
}

#[cfg(test)]
#[path = "lsp_symbols_tests.rs"]
mod tests;
