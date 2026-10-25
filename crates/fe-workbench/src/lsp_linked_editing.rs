//! Linked editing, as VS Code's `editor.linkedEditing` does: with the
//! setting on, once the active editor's caret has rested [`REST`] outside
//! any linked range, the server is asked for the ranges linked to the one
//! at the caret (`textDocument/linkedEditingRange`). If two or more come
//! back and one holds the caret, the editor links them
//! (`EditorModel::set_linked_ranges`), so typing in an HTML tag's name
//! renames its pair; else it links nothing. The server's `wordPattern` is
//! ignored: the model's fixed rule applies.
//!
//! An answer for an old caret or version is dropped. The setting turning
//! off, another editor becoming active, or the server stopping clears the
//! link; the model ends it itself when the caret leaves the ranges.

use std::time::Duration;

use fe_buffer::Version;
use fe_gpui::EditorView;
use fe_lsp::LinkedEditing;
use gpui::{App, Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long the caret rests before the ranges are asked for: VS Code's
/// `editor.linkedEditing` delay is the same as for highlights.
const REST: Duration = Duration::from_millis(250);

/// Where the ranges are asked for: the editor, its text's version and the
/// caret.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Wanted {
    editor: EntityId,
    version: Version,
    at: usize,
}

/// The active editor's linked editing.
#[derive(Default)]
pub(super) struct Linked {
    wanted: Option<Wanted>,
    /// The editor the ranges were linked on, while they are.
    linked_on: Option<Entity<EditorView>>,
    /// The wait for the caret to rest, then the request.
    task: Option<Task<()>>,
}

impl Workbench {
    /// Where `editor` asks for linked ranges now: nothing unless the caret
    /// is the only selection, and is outside any range already linked.
    fn linked_wanted(&self, editor: &Entity<EditorView>, cx: &App) -> Option<Wanted> {
        let model = editor.read(cx).model();
        // Only a buffer on a server is asked about.
        super::lock(self.lsp.manager.as_ref()?).server_of(model.buffer().id())?;
        if model.selections().len() != 1 {
            return None;
        }
        let selection = model.primary_selection();
        if !selection.range.is_empty() {
            return None;
        }
        let at = selection.range.start;
        let held = (model.linked_ranges().into_iter().flatten())
            .any(|range| range.start <= at && at <= range.end);
        if held {
            return None;
        }
        Some(Wanted {
            editor: editor.entity_id(),
            version: model.buffer().version(),
            at,
        })
    }

    /// Ends the link on the editor it was made on, if any.
    fn clear_linked(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.lsp.linked.linked_on.take() {
            editor.update(cx, |view, _| view.model_mut().clear_linked_ranges());
        }
    }

    /// Brings linked editing up to date with the active editor's caret:
    /// run by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_linked_editing(&mut self, cx: &mut Context<Self>) {
        let active = self.active_editor(cx).cloned();
        if self.lsp.manager.is_none() || !self.lsp.linked_editing_on.get() {
            self.lsp.linked.wanted = None;
            self.lsp.linked.task = None;
            self.clear_linked(cx);
            return;
        }
        let served = |editor: &Entity<EditorView>| {
            let buffer = editor.read(cx).model().buffer().id();
            (self.lsp.manager.as_ref())
                .is_some_and(|manager| super::lock(manager).server_of(buffer).is_some())
        };
        let stopped = (self.lsp.linked.linked_on.as_ref()).is_some_and(|editor| !served(editor));
        if stopped || self.lsp.linked.linked_on != active {
            self.clear_linked(cx);
        }
        let now = active
            .as_ref()
            .and_then(|editor| self.linked_wanted(editor, cx));
        if self.lsp.linked.wanted == now {
            return;
        }
        self.lsp.linked.wanted = now.clone();
        self.lsp.linked.task = None;
        let Some(wanted) = now else {
            return;
        };
        let task = cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(REST).await;
            let Ok(Some(reply)) =
                workbench.update(cx, |workbench, cx| workbench.ask_linked(&wanted, cx))
            else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.linked_landed(&wanted, answer, cx)
                })
                .ok();
        });
        self.lsp.linked.task = Some(task);
    }

    /// The request for `wanted`, sent now: nothing if it isn't current.
    fn ask_linked(
        &self,
        wanted: &Wanted,
        cx: &App,
    ) -> Option<impl Future<Output = Result<Option<LinkedEditing>, fe_lsp::LspError>> + use<>> {
        let editor = self.active_editor(cx)?;
        if self.linked_wanted(editor, cx).as_ref() != Some(wanted) {
            return None;
        }
        let buffer = editor.read(cx).model().buffer().id();
        let manager = super::lock(self.lsp.manager.as_ref()?);
        Some(manager.linked_editing_ranges(buffer, wanted.version, wanted.at))
    }

    /// Links the ranges `answer` holds, if the editor is still active with
    /// the same caret and version; else, or if the answer has no pair
    /// around the caret, links nothing.
    fn linked_landed(
        &mut self,
        wanted: &Wanted,
        answer: Result<Option<LinkedEditing>, fe_lsp::LspError>,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if self.linked_wanted(&editor, cx).as_ref() != Some(wanted) {
            return;
        }
        let ranges = match answer {
            Ok(Some(found)) => found.ranges,
            Ok(None) => Vec::new(),
            Err(error) => {
                log::debug!("no linked editing ranges: {error}");
                Vec::new()
            }
        };
        let linkable = ranges.len() >= 2
            && ranges
                .iter()
                .any(|range| range.start <= wanted.at && wanted.at <= range.end);
        self.clear_linked(cx);
        if !linkable {
            return;
        }
        editor.update(cx, |view, _| view.model_mut().set_linked_ranges(ranges));
        self.lsp.linked.linked_on = Some(editor);
    }
}

#[cfg(test)]
#[path = "lsp_linked_editing_tests.rs"]
mod tests;
