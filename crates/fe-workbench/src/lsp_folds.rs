//! Language servers' folding ranges in place of the syntax tree's, as VS
//! Code folds with its language's provider: asked for when a buffer opens
//! on its server and 400 ms after its text last changed. A non-empty answer
//! is the buffer's editors' fold model ([`EditorView::set_fold_ranges`]);
//! `None` (no `foldingRangeProvider`), an empty answer or an error puts the
//! tree back. Nothing is asked until the server has the document open. An answer is shown only if it is for the buffer's current
//! version, from the opening the server still has; until one lands the old
//! ranges stay, their rows following the edits.

use std::{collections::HashMap, ops::RangeInclusive, time::Duration};

use fe_buffer::{BufferId, Version};
use fe_gpui::EditorView;
use fe_lsp::{FoldingRange, IncarnationId, LspError, ServerId, SessionId};
use gpui::{Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long a buffer holds still before its folding ranges are asked for
/// again.
const REST: Duration = Duration::from_millis(400);

/// The opening of a document a server has: its session and incarnation.
type Identity = (SessionId, IncarnationId);

/// What a request answers.
type Answer = Result<Option<Vec<FoldingRange>>, LspError>;

struct Slot {
    /// The editors of the buffer.
    editors: HashMap<EntityId, Entity<EditorView>>,
    /// The server the buffer is on.
    server: ServerId,
    /// The opening the server had when ranges were last asked for.
    identity: Option<Identity>,
    /// The version last seen, to ask again when it changes.
    key: Option<Version>,
    /// The version of the latest request.
    asked: Option<Version>,
    /// The server that said it has no provider.
    no_provider: bool,
    /// Whether the editors fold with the server's ranges.
    applied: bool,
    /// The wait, then the request.
    task: Option<Task<()>>,
}

/// Every open buffer's folding requests.
#[derive(Default)]
pub(super) struct Folds {
    slots: HashMap<BufferId, Slot>,
}

impl Workbench {
    /// Brings each buffer's folding ranges up to date with its text and
    /// server: run by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_folds(&mut self, cx: &mut Context<Self>) {
        let mut wanted: HashMap<BufferId, (Version, Vec<Entity<EditorView>>)> = HashMap::new();
        for editor in self.open_editors(cx) {
            let model = editor.read(cx).model();
            let (buffer, version) = (model.buffer().id(), model.snapshot().version());
            wanted
                .entry(buffer)
                .or_insert_with(|| (version, Vec::new()))
                .1
                .push(editor);
        }
        let gone: Vec<BufferId> = (self.lsp.folds.slots.keys())
            .filter(|buffer| !wanted.contains_key(buffer))
            .copied()
            .collect();
        for buffer in gone {
            self.drop_folds(buffer, cx);
        }
        for (buffer, (version, editors)) in wanted {
            let server = (self.lsp.manager.as_ref())
                .and_then(|manager| super::lock(manager).server_of(buffer));
            let Some(server) = server else {
                self.drop_folds(buffer, cx);
                continue;
            };
            let identity = (self.lsp.manager.as_ref())
                .and_then(|manager| super::lock(manager).active_identity(buffer, server));
            // A move to another server retires the old one's ranges.
            if (self.lsp.folds.slots.get(&buffer)).is_some_and(|slot| slot.server != server) {
                self.drop_folds(buffer, cx);
            }
            let slot = self.lsp.folds.slots.entry(buffer).or_insert_with(|| Slot {
                editors: HashMap::new(),
                server,
                identity,
                key: None,
                asked: None,
                no_provider: false,
                applied: false,
                task: None,
            });
            // Another opening of the document on the server asks afresh,
            // and the old opening's ranges and request are retired.
            if slot.identity != identity {
                slot.identity = identity;
                slot.key = None;
                slot.asked = None;
                slot.no_provider = false;
                slot.task = None;
                if std::mem::take(&mut slot.applied) {
                    for editor in slot.editors.values() {
                        editor.update(cx, |view, cx| {
                            view.set_fold_ranges(None);
                            cx.notify();
                        });
                    }
                }
            }
            slot.editors
                .retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
            for editor in &editors {
                if slot
                    .editors
                    .insert(editor.entity_id(), editor.clone())
                    .is_none()
                {
                    // A new editor has the tree's folds until an answer.
                    slot.key = None;
                }
            }
            // Until the server has the document open there's no opening to
            // ask of: a request sent now would be retired, yet still go out,
            // once the opening appears.
            if slot.no_provider || slot.key == Some(version) || identity.is_none() {
                continue;
            }
            // The first ask goes at once; later ones wait for the text to
            // hold still.
            let delay = if slot.key.is_none() {
                Duration::ZERO
            } else {
                REST
            };
            slot.key = Some(version);
            self.ask_folds_in(buffer, delay, cx);
        }
    }

    /// Gives `buffer`'s editors back the tree's folds and forgets it.
    fn drop_folds(&mut self, buffer: BufferId, cx: &mut Context<Self>) {
        let Some(slot) = self.lsp.folds.slots.remove(&buffer) else {
            return;
        };
        if slot.applied {
            for editor in slot.editors.values() {
                editor.update(cx, |view, cx| {
                    view.set_fold_ranges(None);
                    cx.notify();
                });
            }
        }
    }

    /// Has `buffer` ask for its folding ranges after `delay`, in place of
    /// any request waiting or in flight.
    fn ask_folds_in(&mut self, buffer: BufferId, delay: Duration, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |workbench, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some((version, identity, reply))) =
                workbench.update(cx, |workbench, cx| workbench.send_folds(buffer, cx))
            else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.folds_landed(buffer, version, identity, answer, cx)
                })
                .ok();
        });
        if let Some(slot) = self.lsp.folds.slots.get_mut(&buffer) {
            slot.task = Some(task);
        }
    }

    /// The request for `buffer`'s folding ranges at its version now, sent.
    #[allow(clippy::type_complexity)]
    fn send_folds(
        &mut self,
        buffer: BufferId,
        cx: &mut Context<Self>,
    ) -> Option<(
        Version,
        Option<Identity>,
        std::pin::Pin<Box<dyn std::future::Future<Output = Answer> + Send>>,
    )> {
        let manager = self.lsp.manager.clone()?;
        let slot = self.lsp.folds.slots.get_mut(&buffer)?;
        let editor = slot.editors.values().next()?;
        let version = editor.read(cx).model().snapshot().version();
        slot.asked = Some(version);
        let manager = super::lock(&manager);
        let identity = manager.active_identity(buffer, slot.server);
        let reply = manager.folding_ranges(buffer, version);
        Some((version, identity, Box::pin(reply)))
    }

    /// Applies `answer`, the reply to the request for `buffer` at
    /// `version`, if it is still the latest, from the opening the server
    /// still has, and the text hasn't moved on.
    fn folds_landed(
        &mut self,
        buffer: BufferId,
        version: Version,
        identity: Option<Identity>,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.lsp.folds.slots.get_mut(&buffer) else {
            return;
        };
        let current = (self.lsp.manager.as_ref())
            .and_then(|manager| super::lock(manager).active_identity(buffer, slot.server));
        let text_is = slot
            .editors
            .values()
            .next()
            .is_some_and(|editor| editor.read(cx).model().snapshot().version() == version);
        if slot.asked != Some(version) || identity.is_none() || identity != current || !text_is {
            return;
        }
        let ranges: Option<Vec<RangeInclusive<u32>>> = match &answer {
            Ok(Some(found)) if !found.is_empty() => Some(
                (found.iter())
                    .map(|range| range.start_row..=range.end_row)
                    .collect(),
            ),
            _ => None,
        };
        if matches!(answer, Ok(None)) {
            slot.no_provider = true;
        }
        if let Err(err) = &answer {
            log::debug!("folding ranges failed: {err}");
        }
        slot.applied = ranges.is_some();
        for editor in slot.editors.values() {
            let ranges = ranges.clone();
            editor.update(cx, |view, cx| {
                view.set_fold_ranges(ranges);
                cx.notify();
            });
        }
    }
}

#[cfg(test)]
#[path = "lsp_folds_tests.rs"]
mod tests;
