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
use gpui::{Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long the buffer and the rows in view hold still before hints are
/// asked for.
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
                    editor.update(cx, |view, cx| {
                        view.clear_inlay_hints();
                        view.set_inlay_hint_host(Some(host));
                        cx.notify();
                    });
                }
            }
            let Some(server) = server else {
                if slot.server.take().is_some() {
                    slot.key = None;
                    slot.asked = None;
                    slot.no_provider = None;
                    slot.task = None;
                    editor.update(cx, |view, cx| {
                        view.clear_inlay_hints();
                        cx.notify();
                    });
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
                    editor.update(cx, |view, cx| {
                        view.clear_inlay_hints();
                        cx.notify();
                    });
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
        let editor = slot.editor.clone();
        match answer {
            Ok(Some(found)) => {
                let hints = found.into_iter().map(convert).collect();
                editor.update(cx, |view, cx| {
                    view.set_inlay_hints(version, hints);
                    cx.notify();
                });
            }
            Ok(None) => {
                slot.no_provider = slot.server;
                editor.update(cx, |view, cx| {
                    view.clear_inlay_hints();
                    cx.notify();
                });
            }
            Err(error) => log::debug!("no inlay hints: {error}"),
        }
    }
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
