//! Language servers' inlay hints in editors, as VS Code asks for them: the
//! hints of the rows in view, a viewport above and below included, 250 ms
//! after the buffer or the rows in view last changed, and at once when the
//! server asks for a refresh. `editor.inlay_hints` off shows none and asks
//! for none.
//!
//! An answer lands only if it is for the latest request the editor made; a
//! server without a provider is not asked again until the buffer's server
//! changes.

use std::{
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use fe_buffer::Version;
use fe_gpui::{EditorView, InlayHint, InlayHintFuture, InlayHintHost, InlayHintKind};
use fe_lsp::LspManager;
use fe_lsp::{LspError, ServerId};
use gpui::{AppContext as _, Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long the buffer and the rows in view hold still before hints are
/// asked for.
/// Texts up to this many bytes have their outline found on the main thread.
const INLINE_OUTLINE_BYTES: usize = 256 * 1024;

const REST: Duration = Duration::from_millis(250);

/// What was asked for last: the buffer version and the rows in view.
type Key = (Version, (u32, u32));

/// What a request answers.
type Answer = Result<Option<Vec<fe_lsp::InlayHint>>, LspError>;

struct Slot {
    editor: Entity<EditorView>,
    /// The server the buffer is on while hints are wanted.
    server: Option<ServerId>,
    /// The version and rows last seen, to ask again when they change.
    key: Option<Key>,
    /// The version and byte range of the latest request.
    asked: Option<(Version, Range<usize>)>,
    /// The server that said it has no provider.
    no_provider: Option<ServerId>,
    /// The wait, then the request.
    task: Option<Task<()>>,
    /// The manager the editor's host resolves through.
    hosted: Option<Weak<Mutex<LspManager>>>,
    /// The server's hints and the version they're of.
    served: Option<(Version, Vec<InlayHint>)>,
    /// The closing-brace hints from the outline and the version they're of.
    closing: Option<(Version, Vec<InlayHint>)>,
    /// What the closing hints were last asked for: the version, whether
    /// they're on and the fewest rows.
    closing_key: Option<(Version, bool, u32)>,
    /// The outline being found.
    closing_task: Option<Task<()>>,
}

/// Every open editor's hints.
#[derive(Default)]
pub(super) struct Slots {
    slots: HashMap<EntityId, Slot>,
}

impl Workbench {
    /// Brings each editor's hints up to date with its buffer and the rows
    /// in view: run by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_inlay_hints(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let on = self.lsp.inlay_hints_on.get();
        self.lsp
            .inlay_hints
            .slots
            .retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
        for editor in &editors {
            let id = editor.entity_id();
            let (buffer, version) = {
                let model = editor.read(cx).model();
                (model.buffer().id(), model.snapshot().version())
            };
            let server = self
                .lsp
                .manager
                .as_ref()
                .and_then(|manager| super::lock(manager).server_of(buffer))
                .filter(|_| on);
            let rows = editor.read(cx).viewport_rows();
            let slot = self
                .lsp
                .inlay_hints
                .slots
                .entry(id)
                .or_insert_with(|| Slot {
                    editor: editor.clone(),
                    server: None,
                    key: None,
                    asked: None,
                    no_provider: None,
                    task: None,
                    hosted: None,
                    served: None,
                    closing: None,
                    closing_key: None,
                    closing_task: None,
                });
            // A new manager (the root changed) doesn't know the old one's
            // hints: they go, and the host follows.
            if let Some(manager) = &self.lsp.manager {
                let manager = Arc::downgrade(manager);
                if !slot
                    .hosted
                    .as_ref()
                    .is_some_and(|w| Weak::ptr_eq(w, &manager))
                {
                    if slot.hosted.replace(manager.clone()).is_some() {
                        slot.server = None;
                        slot.key = None;
                        slot.asked = None;
                        slot.no_provider = None;
                        slot.task = None;
                    }
                    let host = Rc::new(Host { manager });
                    slot.served = None;
                    editor.update(cx, |view, _| {
                        view.set_inlay_hint_host(Some(host));
                    });
                    publish(slot, cx);
                }
            }
            let Some(server) = server else {
                if slot.server.take().is_some() {
                    slot.key = None;
                    slot.asked = None;
                    slot.no_provider = None;
                    slot.task = None;
                    slot.served = None;
                    publish(slot, cx);
                }
                continue;
            };
            if slot.server != Some(server) {
                // Straight from another server: its hints and its request
                // in flight go.
                let moved = slot.server.replace(server).is_some();
                slot.key = None;
                slot.no_provider = None;
                if moved {
                    slot.asked = None;
                    slot.task = None;
                    slot.served = None;
                    publish(slot, cx);
                }
            }
            if slot.no_provider == Some(server) {
                continue;
            }
            let Some(rows) = rows else { continue };
            let key = (version, rows);
            if slot.key == Some(key) {
                continue;
            }
            slot.key = Some(key);
            self.ask_inlay_hints_in(id, REST, cx);
        }
        self.sync_closing_hints(&editors, cx);
    }

    /// Finds each editor's closing-brace hints from its outline when its
    /// parsed text or the settings changed, and shows them with the server's.
    fn sync_closing_hints(&mut self, editors: &[Entity<EditorView>], cx: &mut Context<Self>) {
        let (on, min_rows) = self.lsp.closing_hints.get();
        for editor in editors {
            let id = editor.entity_id();
            let Some(slot) = self.lsp.inlay_hints.slots.get_mut(&id) else {
                continue;
            };
            let view = editor.read(cx);
            let version = view.model().snapshot().version();
            let key = (version, on, min_rows);
            let len = view.model().snapshot().text().len();
            if slot.closing_key == Some(key) {
                continue;
            }
            let job = if !on {
                None
            } else if let Some(syntax) = view.syntax() {
                match fe_syntax::outline_job(syntax) {
                    Ok(job) => Some(job),
                    // Reparsed soon, and drawn again then.
                    Err(fe_syntax::NoOutline::NotParsed) => continue,
                    Err(_) => None,
                }
            } else if view.is_loading_language() {
                // Its grammar lands later, and the editor is drawn again.
                continue;
            } else {
                None
            };
            slot.closing_key = Some(key);
            let Some(job) = job else {
                slot.closing_task = None;
                if slot.closing.take().is_some() {
                    publish(slot, cx);
                }
                continue;
            };
            // Small texts' outlines take a few milliseconds: found here, so
            // no job is in flight when the window closes. Big ones go to
            // the background.
            let (found, inline) = if len > INLINE_OUTLINE_BYTES {
                (Some(cx.background_spawn(async move { job.run() })), None)
            } else {
                (None, Some(job))
            };
            slot.closing_task = Some(cx.spawn(async move |workbench, cx| {
                let outline = match (found, inline) {
                    (Some(found), _) => found.await,
                    (None, Some(job)) => job.run(),
                    (None, None) => return,
                };
                workbench
                    .update(cx, |workbench, cx| {
                        workbench.closing_hints_landed(id, key, outline, cx)
                    })
                    .ok();
            }));
        }
    }

    /// Shows `outline`'s closing-brace hints if the editor is still at the
    /// version and settings they were asked for at.
    fn closing_hints_landed(
        &mut self,
        id: EntityId,
        key: (Version, bool, u32),
        outline: fe_syntax::Outline,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.lsp.inlay_hints.slots.get_mut(&id) else {
            return;
        };
        if slot.closing_key != Some(key) {
            return;
        }
        let view = slot.editor.read(cx);
        let snapshot = view.model().snapshot();
        if snapshot.version() != key.0 || outline.version != key.0 {
            // Parsed at another version: asked for again at the next frame.
            slot.closing_key = None;
            return;
        }
        let hints = crate::closing_brace_hints::hints(&outline, snapshot.text(), key.2);
        // Nothing to show, and nothing shown: the editor is left alone.
        let unchanged =
            hints.is_empty() && slot.closing.as_ref().is_none_or(|(_, old)| old.is_empty());
        slot.closing = Some((key.0, hints));
        if !unchanged {
            publish(slot, cx);
        }
    }

    /// Asks again, at once, for every editor whose buffer is on `server`.
    pub(super) fn refresh_inlay_hints(&mut self, server: ServerId, cx: &mut Context<Self>) {
        let ids: Vec<EntityId> = (self.lsp.inlay_hints.slots.iter())
            .filter(|(_, slot)| slot.server == Some(server))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(slot) = self.lsp.inlay_hints.slots.get_mut(&id) {
                slot.no_provider = None;
            }
            self.ask_inlay_hints_in(id, Duration::ZERO, cx);
        }
    }

    /// Has editor `id` ask for its hints after `delay`, in place of any
    /// request waiting or in flight.
    fn ask_inlay_hints_in(&mut self, id: EntityId, delay: Duration, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |workbench, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some((version, range, reply))) =
                workbench.update(cx, |workbench, cx| workbench.send_inlay_hints(id, cx))
            else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.inlay_hints_landed(id, version, range, answer, cx)
                })
                .ok();
        });
        if let Some(slot) = self.lsp.inlay_hints.slots.get_mut(&id) {
            slot.task = Some(task);
        }
    }

    /// The request for editor `id`'s rows in view now, sent.
    #[allow(clippy::type_complexity)]
    fn send_inlay_hints(
        &mut self,
        id: EntityId,
        cx: &mut Context<Self>,
    ) -> Option<(
        Version,
        Range<usize>,
        std::pin::Pin<Box<dyn std::future::Future<Output = Answer> + Send>>,
    )> {
        let manager = self.lsp.manager.clone()?;
        let slot = self.lsp.inlay_hints.slots.get_mut(&id)?;
        let view = slot.editor.read(cx);
        let (top, rows) = view.viewport_rows()?;
        let model = view.model();
        let snapshot = model.snapshot();
        let (buffer, version) = (model.buffer().id(), snapshot.version());
        // The rows in view and a viewport more each way.
        let text = snapshot.text();
        let offset = |row: u32| {
            if row >= text.row_count() {
                text.len()
            } else {
                text.row_start(row)
            }
        };
        let range =
            offset(top.saturating_sub(rows))..offset(top.saturating_add(rows.saturating_mul(2)));
        slot.asked = Some((version, range.clone()));
        let reply = super::lock(&manager).inlay_hints(buffer, version, range.clone());
        Some((version, range, Box::pin(reply)))
    }

    /// Shows `answer`, the reply to the request for `range` at `version`
    /// of editor `id`, if it is still the latest.
    fn inlay_hints_landed(
        &mut self,
        id: EntityId,
        version: Version,
        range: Range<usize>,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.lsp.inlay_hints.slots.get_mut(&id) else {
            return;
        };
        if slot.asked.as_ref() != Some(&(version, range)) || !self.lsp.inlay_hints_on.get() {
            return;
        }
        match answer {
            Ok(Some(found)) => {
                slot.served = Some((version, found.into_iter().map(convert).collect()));
                publish(slot, cx);
            }
            Ok(None) => {
                slot.no_provider = slot.server;
                slot.served = None;
                publish(slot, cx);
            }
            Err(error) => log::debug!("no inlay hints: {error}"),
        }
    }
}

/// Shows the editor's server hints and closing-brace hints together: at
/// the server's version, with the closing hints if they're of it too; a
/// closing hint a server hint duplicates is left out.
fn publish(slot: &Slot, cx: &mut Context<Workbench>) {
    let mut shown: Option<(Version, Vec<InlayHint>)> = slot.served.clone();
    if let Some((version, closing)) = &slot.closing {
        match &mut shown {
            None => shown = Some((*version, closing.clone())),
            Some((served_version, hints)) if served_version == version => {
                let extra = crate::closing_brace_hints::without_served(closing, hints);
                hints.extend(extra);
            }
            Some(_) => {}
        }
    }
    slot.editor.update(cx, |view, cx| {
        match shown {
            Some((version, hints)) => view.set_inlay_hints(version, hints),
            None => view.clear_inlay_hints(),
        }
        cx.notify();
    });
}

/// Resolves an editor's hints through the language servers.
struct Host {
    manager: Weak<Mutex<LspManager>>,
}

impl InlayHintHost for Host {
    fn resolve(&self, hint: InlayHint) -> InlayHintFuture {
        let manager = self.manager.upgrade();
        Box::pin(async move {
            let Some(manager) = manager else { return hint };
            let Some(listed) = (hint.data.as_ref())
                .and_then(|data| data.downcast_ref::<fe_lsp::InlayHint>())
                .cloned()
            else {
                return hint;
            };
            // Sent with the lock held, and awaited without it.
            let reply = super::lock(&manager).resolve_inlay_hint(listed);
            drop(manager);
            match reply.await {
                Ok(resolved) => InlayHint {
                    offset: hint.offset,
                    ..convert(resolved)
                },
                Err(error) => {
                    log::debug!("inlay hint not resolved: {error}");
                    hint
                }
            }
        })
    }
}

/// `hint` as an editor draws it, keeping the server's in `data`.
fn convert(hint: fe_lsp::InlayHint) -> InlayHint {
    InlayHint {
        offset: hint.offset,
        label: hint.label.clone(),
        kind: hint.kind.map(|kind| match kind {
            fe_lsp::InlayHintKind::Type => InlayHintKind::Type,
            fe_lsp::InlayHintKind::Parameter => InlayHintKind::Parameter,
        }),
        padding_left: hint.padding_left,
        padding_right: hint.padding_right,
        tooltip: hint.tooltip.clone(),
        edits: hint.edits.clone(),
        data: Some(Rc::new(hint)),
    }
}
