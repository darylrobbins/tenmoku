//! The menu's Code Actions… item (T1898): shown only where the server
//! offers some. The menu opens without it; as it does, the buffer's servers
//! are asked for the actions at the editor's primary selection (a caret at
//! the click, or the selection the click was in), as `lsp.code_actions`
//! asks (`triggerKind` invoked, no kind, the diagnostics over it). If an
//! enabled action other than a `source` one comes back, the item goes in
//! with the language server's items ([`placement`]): right after Rename
//! Symbol; without it, at the end of the Format items' group; without
//! those, in a group of its own after the navigation items; with none of
//! them (a server offering code actions and nothing else the menu lists
//! here), as the menu's first group, ahead of Cut, Copy and Paste.
//!
//! An answer adds nothing unless the menu it was asked for is still open on
//! the same buffer, version and selection; a failed or empty one, nothing.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_gpui::{EditorMenuItem, EditorMenuRequest, EditorMenuTicket, EditorView};
use fe_lsp::{CodeAction, code_action::CodeActionQuery};
use gpui::{App, Context, Entity};
use lsp_types::ServerCapabilities;

use super::{Item, NAVIGATION};
use crate::{Workbench, lsp_code_actions::sent_diagnostic};

/// Where the actions were asked for: the menu, and the buffer, version and
/// selection at its opening.
struct Asked {
    ticket: EditorMenuTicket,
    buffer: BufferId,
    version: Version,
    range: Range<usize>,
}

impl Asked {
    /// Whether `view` still shows the menu asked for, over what it was.
    fn current(&self, view: &EditorView) -> bool {
        let model = view.model();
        view.editor_menu_ticket() == Some(self.ticket)
            && model.buffer().id() == self.buffer
            && model.buffer().version() == self.version
            && model.primary_selection().range == self.range
    }
}

/// How many answers to the menu's requests have been handled, for tests.
#[cfg(test)]
#[derive(Default)]
struct Answered(usize);

#[cfg(test)]
impl gpui::Global for Answered {}

/// How many answers to the menu's code action requests have been handled
/// in the app, added or not.
#[cfg(test)]
pub(crate) fn answered(cx: &App) -> usize {
    cx.try_global::<Answered>().map_or(0, |answered| answered.0)
}

/// Where the item goes in a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Placement {
    /// Right after the item running this command, in its group, or
    /// starting a group of its own there.
    After { command: String, own_group: bool },
    /// As a group of its own right before the item running this command:
    /// the menu's first.
    Before(String),
}

/// Where the item goes in a menu of `entries`: see the [module](self).
/// `None` for a menu with no item.
fn placement(entries: &[Option<EditorMenuItem>]) -> Option<Placement> {
    let has = |item: Item| {
        (entries.iter().flatten()).any(|entry| entry.invocation.command.as_str() == item.command())
    };
    let after = |item: Item, own_group| Placement::After {
        command: item.command().to_owned(),
        own_group,
    };
    if has(Item::Rename) {
        return Some(after(Item::Rename, false));
    }
    let last = |items: &[Item]| items.iter().rev().copied().find(|&item| has(item));
    if let Some(format) = last(&[Item::FormatDocument, Item::FormatSelection]) {
        return Some(after(format, false));
    }
    if let Some(navigation) = last(NAVIGATION) {
        return Some(after(navigation, true));
    }
    let first = entries.iter().flatten().next()?;
    Some(Placement::Before(
        first.invocation.command.as_str().to_owned(),
    ))
}

/// Whether `actions` call for the item: an enabled one that isn't a
/// `source` one, as the lightbulb counts them.
fn worth_listing(actions: &[CodeAction]) -> bool {
    (actions.iter()).any(|action| action.disabled.is_none() && !action.kind_is("source"))
}

/// Whether a diagnostic's range is one `selection` is at: a caret is at the
/// ends of a range too, a selection must share some of it.
fn overlaps(diagnostic: &Range<usize>, selection: &Range<usize>) -> bool {
    match selection.is_empty() {
        true => diagnostic.start <= selection.start && selection.start <= diagnostic.end,
        false => diagnostic.start < selection.end && selection.start < diagnostic.end,
    }
}

impl Workbench {
    /// Asks `editor`'s servers, whose `capabilities` these are, for the
    /// actions at its selection, for the menu `request` opens, if one has
    /// a code action provider; see the [module](self).
    pub(super) fn ask_editor_menu_code_actions(
        &self,
        request: &EditorMenuRequest,
        editor: &Entity<EditorView>,
        capabilities: &[ServerCapabilities],
        entries: &[Option<EditorMenuItem>],
        cx: &mut Context<Self>,
    ) {
        if !(capabilities.iter()).any(|caps| Item::CodeActions.provided_by(caps)) {
            return;
        }
        let Some(placement) = placement(entries) else {
            return;
        };
        let model = editor.read(cx).model();
        let asked = Asked {
            ticket: request.ticket,
            buffer: model.buffer().id(),
            version: model.buffer().version(),
            range: model.primary_selection().range,
        };
        let diagnostics = (model.diagnostics().into_iter())
            .filter(|entry| overlaps(&entry.range, &asked.range))
            .map(|entry| sent_diagnostic(&entry))
            .collect();
        let Some(actioner) = self.lsp_code_actioner(asked.buffer) else {
            return;
        };
        let query = CodeActionQuery {
            only: None,
            automatic: false,
        };
        let reply = actioner.ask(
            asked.buffer,
            asked.version,
            asked.range.clone(),
            diagnostics,
            query,
        );
        let editor = editor.downgrade();
        let contexts = request.contexts.clone();
        cx.spawn(async move |_, cx| {
            let answer = reply.await;
            cx.update(|cx: &mut App| {
                #[cfg(test)]
                {
                    cx.default_global::<Answered>().0 += 1;
                }
                let Some(editor) = editor.upgrade() else {
                    return;
                };
                if !asked.current(editor.read(cx)) {
                    return;
                }
                match answer {
                    Ok(actions) if worth_listing(&actions) => {}
                    Ok(_) => return,
                    Err(error) => {
                        log::debug!("no code actions for the editor menu: {error}");
                        return;
                    }
                }
                let item = Some(Item::CodeActions.entry(true, &contexts, cx));
                editor.update(cx, |view, cx| match placement {
                    Placement::After { command, own_group } => {
                        let items = match own_group {
                            true => vec![None, item],
                            false => vec![item],
                        };
                        view.extend_editor_menu(asked.ticket, &command, items, cx)
                    }
                    Placement::Before(command) => {
                        view.extend_editor_menu_before(asked.ticket, &command, vec![item, None], cx)
                    }
                });
            });
        })
        .detach();
    }
}
