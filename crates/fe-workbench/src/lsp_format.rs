//! Formatting the active editor's document or selection through its
//! language server (`lsp.format_document`, `lsp.format_selection`).
//!
//! - **The request.** For the buffer at its version now: the whole
//!   document, or the primary selection widened to whole rows (a caret
//!   formats its row). The options are the editor's: its tab size and
//!   whether it indents with spaces (its detected or configured style, or
//!   spaces with none).
//! - **The edits.** Applied to the buffer as one transaction of the user,
//!   as accepting a completion is, so the editor's undo takes it back in one
//!   step (it undoes only its own origin, which `Origin::Format` isn't);
//!   selections stay anchored. If the buffer has changed since the request
//!   (the user typed meanwhile, or saved it under another name) they're
//!   dropped, and nothing is shown.
//! - **No result.** A server with no formatter shows "No formatter for
//!   <language>"; a timeout or an error shows itself. Both as info toasts.
//! - **[`Workbench::format_buffer`]** is the same for the document, for
//!   format on save, which reports what happened instead of showing it.

use std::{cell::Cell, ops::Range, path::Path, rc::Rc};

use fe_buffer::{Origin, ProposedChange};
use fe_editor::IndentStyle;
use fe_gpui::{EditorView, SaveHook};
use fe_lsp::{LspError, formatting::FormatOptions};
use gpui::{App, AppContext as _, Context, Entity, Task, WeakEntity, Window};

use crate::{
    Workbench,
    status_bar::{PLAIN_TEXT, language_label},
    workbench::toast::{self, Toast},
};

/// What formatting a buffer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormatOutcome {
    /// Its text changed.
    Formatted,
    /// The server had nothing to change, or the buffer changed while it
    /// answered, so its edits were dropped.
    Unchanged,
    /// The buffer's server has no formatter, or it has no server.
    NoFormatter,
}

impl Workbench {
    /// Formats the active editor's document, or only its selection if
    /// `selection`.
    pub(crate) fn format_active(
        &mut self,
        selection: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let language = (editor.read(cx).syntax())
            .map(|syntax| language_label(syntax.language().name()))
            .unwrap_or_else(|| PLAIN_TEXT.into());
        let task = self.format_range(&editor, selection, cx);
        cx.spawn(async move |_, cx| {
            let message = match task.await {
                Ok(FormatOutcome::Formatted | FormatOutcome::Unchanged) => return,
                Ok(FormatOutcome::NoFormatter) => format!("No formatter for {language}"),
                Err(error) => error,
            };
            cx.update(|cx| toast::show(cx, Toast::info(message)));
        })
        .detach();
    }

    /// Formats `editor`'s whole buffer: see the [module](self).
    pub(crate) fn format_buffer(
        &mut self,
        editor: &Entity<EditorView>,
        cx: &mut Context<Self>,
    ) -> Task<Result<FormatOutcome, String>> {
        self.format_range(editor, false, cx)
    }

    /// Formats `editor`'s buffer, or its selection's rows if `selection`.
    fn format_range(
        &mut self,
        editor: &Entity<EditorView>,
        selection: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<FormatOutcome, String>> {
        let Some(manager) = self.lsp.manager() else {
            return Task::ready(Ok(FormatOutcome::NoFormatter));
        };
        let (buffer, version, path, range, options) = editor.update(cx, |view, _| {
            // Caught up first, so the rows, the selection and the version
            // describe the same text.
            let model = view.model_mut();
            model.sync_display();
            let range = selection.then(|| rows_of(model, model.primary_selection().range));
            let insert_spaces = !matches!(model.indent_style(), Some(IndentStyle::Tabs));
            let buffer = model.buffer();
            let options = FormatOptions {
                tab_size: model.tab_size() as u32,
                insert_spaces,
            };
            let path = model.path().map(Path::to_owned);
            (buffer.id(), buffer.version(), path, range, options)
        });
        let reply = manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .format(buffer, version, range, options);
        let reply = cx.background_spawn(reply);
        let editor = editor.clone();
        cx.spawn(async move |_, cx| {
            let edits = match reply.await {
                Ok(Some(edits)) => edits,
                Ok(None) => return Ok(FormatOutcome::NoFormatter),
                Err(error) => return failure(error),
            };
            let outcome = editor.update(cx, |view, cx| {
                let model = view.model_mut();
                model.sync_display();
                // Saved under another name since: the edits were for the
                // document under the old one.
                if model.path() != path.as_deref() {
                    return FormatOutcome::Unchanged;
                }
                let mut buffer = model.buffer_mut();
                if buffer.version() != version {
                    return FormatOutcome::Unchanged;
                }
                // Aside from any transaction the user has open (a Vim
                // insert), so undo takes back the format alone.
                let mut change = ProposedChange::from_edits(
                    buffer.current(),
                    merge_inserts(edits),
                    Origin::User,
                );
                let (applied, _) = change.accept_all(&mut buffer);
                drop(buffer);
                cx.notify();
                match applied.is_some() {
                    true => FormatOutcome::Formatted,
                    false => FormatOutcome::Unchanged,
                }
            });
            Ok(outcome)
        })
    }
}

/// A request's failure: the buffer having changed before the server took
/// the request is the user typing, which drops the format like a stale
/// reply, not an error to show.
fn failure(error: LspError) -> Result<FormatOutcome, String> {
    match error {
        LspError::Unavailable(why) if why == "buffer changed" => Ok(FormatOutcome::Unchanged),
        error => Err(error.to_string()),
    }
}

/// Formats an editor before it's saved, when `editor.format_on_save` is on:
/// a timeout, an error or no formatter is logged, never shown.
pub(crate) struct FormatOnSave {
    workbench: WeakEntity<Workbench>,
    enabled: Rc<Cell<bool>>,
}

impl FormatOnSave {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, enabled: Rc<Cell<bool>>) -> Self {
        Self { workbench, enabled }
    }
}

impl SaveHook for FormatOnSave {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()> {
        if !self.enabled.get() {
            return Task::ready(());
        }
        let editor = editor.clone();
        let task = self
            .workbench
            .update(cx, |workbench, cx| workbench.format_buffer(&editor, cx));
        let Ok(task) = task else {
            return Task::ready(());
        };
        cx.spawn(async move |_| match task.await {
            Ok(FormatOutcome::Formatted | FormatOutcome::Unchanged) => {}
            Ok(FormatOutcome::NoFormatter) => log::debug!("format on save: no formatter"),
            Err(error) => log::debug!("format on save: {error}"),
        })
    }
}

/// `edits`, sorted and apart, with inserts at one place joined in their
/// order: a [`ProposedChange`] takes no two at one place.
fn merge_inserts(edits: Vec<(Range<usize>, String)>) -> Vec<(Range<usize>, String)> {
    let mut merged: Vec<(Range<usize>, String)> = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        match merged.last_mut() {
            Some((last, last_text)) if last.start == last.end && range == *last => {
                last_text.push_str(&text);
            }
            _ => merged.push((range, text)),
        }
    }
    merged
}

/// `range` of `model`'s text widened to whole rows, line breaks included.
/// A selection ending at the start of a row doesn't take that row.
fn rows_of(model: &fe_editor::EditorModel, range: Range<usize>) -> Range<usize> {
    let end = match range.end > range.start {
        true => range.end - 1,
        false => range.end,
    };
    model.line_range_at(range.start).start..model.line_range_at(end).end
}

#[cfg(test)]
#[path = "lsp_format_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lsp_format_save_tests.rs"]
mod save_tests;
