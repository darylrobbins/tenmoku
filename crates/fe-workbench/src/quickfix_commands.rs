//! Walking the quickfix and location lists ([`crate::quickfix`]): vim's
//! `:cn`, `:cp`, `:cc`, `:cfir`, `:cla`, `:cnf`, `:cNf`, `:col`, `:cnew`,
//! `:cl` and their `:l` twins ([`fe_vim::QuickfixCommand`], which
//! `vim_host.rs` hands here), and the typed commands over the same code
//! (`workbench_commands.rs`): `quickfix.next`, `quickfix.previous`,
//! `quickfix.first`, `quickfix.last`, `location.next` and
//! `location.previous`.
//!
//! Each window has one quickfix stack, and each of its panes a location
//! stack, which goes when the pane closes; an `:l` command (or a typed
//! `location.*`) uses the active pane's. Landing on an item opens its file
//! in the active pane at its row and column, as Go to Line does
//! ([`Workbench::open_document_at`], a jump), and once it's there says
//! nvim's `(2 of 3) error: text` in the status line: as the vim message of
//! the editor landed in, or with vim off as a toast. Other answers (`E553:
//! No more items`, `:colder`'s `error list 1 of 2; …`, `:clist`'s items)
//! go to the editor the line was typed in, or for a typed command the
//! active editor, the same way.
//!
//! Nothing fills a list yet: `:grep`, `:vimgrep` and `:make` (and their
//! `:l` twins) say they aren't supported. [`Workbench::push_quickfix_list`]
//! is the seam the fillers put their lists through.

use std::{collections::HashMap, path::Path};

use fe_gpui::EditorView;
use fe_vim::{QuickfixAction, QuickfixCommand};
use gpui::{App, Context, Entity, EntityId, Global, Window};
use kiln_shell::WorkspaceEvent;

use crate::{
    Pane, Workbench,
    quickfix::{QuickfixError, QuickfixItem, QuickfixList, QuickfixStack, Quickfixes},
    workbench::toast::{self, Toast},
};

/// What `:grep`, `:vimgrep` and `:make` say until they fill lists.
pub(crate) const NO_FILLING: &str = "Not supported yet: filling a quickfix list";

/// Each window's lists, by its workbench's id, a window's location stacks
/// by their pane's: kept apart from the workbench, and dropped with it.
#[derive(Default)]
struct Lists(HashMap<EntityId, Quickfixes<EntityId>>);

impl Global for Lists {}

/// What a command comes to once its list has answered.
enum Outcome {
    /// The item to open, and what to say once it's open.
    Land(QuickfixItem, String),
    /// A line for the status line.
    Say(String),
}

impl Workbench {
    /// This window's lists, set up on first use to forget a pane's
    /// location stack when the pane goes, and all of them with the window.
    fn quickfixes<'a>(&self, cx: &'a mut Context<Self>) -> &'a mut Quickfixes<EntityId> {
        let id = cx.entity_id();
        if !cx.default_global::<Lists>().0.contains_key(&id) {
            cx.on_release(move |_, cx| {
                cx.default_global::<Lists>().0.remove(&id);
            })
            .detach();
            cx.subscribe(self.workspace(), move |_, _, event, cx| {
                if let WorkspaceEvent::PaneRemoved(pane) = event
                    && let Some(lists) = cx.default_global::<Lists>().0.get_mut(&id)
                {
                    lists.drop_pane(pane);
                }
            })
            .detach();
            cx.default_global::<Lists>()
                .0
                .insert(id, Quickfixes::default());
        }
        cx.default_global::<Lists>()
            .0
            .get_mut(&id)
            .expect("set up above")
    }

    /// Puts `list` on this window's quickfix stack, or with `location` on
    /// the active pane's location stack, after the list in use (dropping
    /// any newer, and past ten the oldest), and uses it, its first item
    /// selected. Nothing is opened.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "lists are filled by the next card's :grep and :make"
        )
    )]
    pub(crate) fn push_quickfix_list(
        &mut self,
        location: bool,
        list: QuickfixList,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane().entity_id();
        let lists = self.quickfixes(cx);
        match location {
            true => lists.push_location(pane, list),
            false => lists.quickfix.push(list),
        }
    }

    /// The list in use: the quickfix stack's, or with `location` the active
    /// pane's location stack's. For the tests.
    #[cfg(test)]
    pub(crate) fn quickfix_list(
        &mut self,
        location: bool,
        cx: &mut Context<Self>,
    ) -> Option<QuickfixList> {
        let pane = self.pane().entity_id();
        let stack = self.quickfixes(cx).stack(location, &pane).ok()?;
        stack.current().cloned()
    }

    /// Whether `pane` has a location stack. For the tests.
    #[cfg(test)]
    pub(crate) fn has_location_list(&mut self, pane: EntityId, cx: &mut Context<Self>) -> bool {
        self.quickfixes(cx).has_location(&pane)
    }

    /// A typed command's `action`, on the location list with `location`,
    /// answering in the active editor.
    pub(crate) fn run_typed_quickfix(
        &mut self,
        action: QuickfixAction,
        location: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.active_editor(cx).cloned();
        let command = QuickfixCommand { action, location };
        self.run_quickfix(command, editor.as_ref(), window, cx);
    }

    /// Runs `command`, typed on the vim line in `editor` (or for a typed
    /// command, the active editor), which hears any answer but the landing
    /// message: see the module docs.
    pub(crate) fn run_quickfix(
        &mut self,
        command: QuickfixCommand,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let QuickfixCommand { action, location } = command;
        if let QuickfixAction::Grep(_) | QuickfixAction::VimGrep(_) | QuickfixAction::Make(_) =
            action
        {
            return say(editor, NO_FILLING, cx);
        }
        let pane = self.pane().entity_id();
        let root = self.root().map(Path::to_owned);
        let lists = self.quickfixes(cx);
        let outcome = lists
            .stack(location, &pane)
            .and_then(|stack| answer(stack, action, root.as_deref()));
        match outcome {
            Err(error) => say(editor, error.to_string(), cx),
            Ok(Outcome::Say(text)) => say(editor, text, cx),
            Ok(Outcome::Land(item, text)) => self.land_on(item, text, window, cx),
        }
    }

    /// Opens `item`'s file in the active pane at its row and column, then
    /// says `text` in the editor there.
    fn land_on(
        &mut self,
        item: QuickfixItem,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = item.row.saturating_add(1);
        let col = item.col.saturating_add(1);
        let path = item.path;
        let opened = self.open_document_at(path.clone(), Some(line), Some(col), window, cx);
        let pane = self.pane().clone();
        cx.spawn(async move |_, cx| {
            opened.await;
            cx.update(|cx| {
                let editor = landed_editor(&pane, &path, cx);
                say(editor.as_ref(), text, cx);
            });
        })
        .detach();
    }
}

/// What `action` comes to on `stack`, whose list it may move through.
fn answer(
    stack: &mut QuickfixStack,
    action: QuickfixAction,
    root: Option<&Path>,
) -> Result<Outcome, QuickfixError> {
    use QuickfixAction::*;
    let moved = match action {
        Older(count) => return stack.older(count).map(Outcome::Say),
        Newer(count) => return stack.newer(count).map(Outcome::Say),
        Grep(_) | VimGrep(_) | Make(_) => return Ok(Outcome::Say(NO_FILLING.to_owned())),
        List => {
            let list = stack.current_mut()?;
            if list.items.is_empty() {
                return Err(QuickfixError::NoErrors);
            }
            return Ok(Outcome::Say(list.listing(root)));
        }
        Next(count) => stack.current_mut()?.next(count),
        Previous(count) => stack.current_mut()?.previous(count),
        NextFile(count) => stack.current_mut()?.next_file(count),
        PreviousFile(count) => stack.current_mut()?.previous_file(count),
        Go(number) => stack.current_mut()?.go(number),
        First => stack.current_mut()?.first(),
        Last => stack.current_mut()?.last(),
    };
    let item = moved?.clone();
    let text = stack.current().map(QuickfixList::landed_message);
    Ok(Outcome::Land(item, text.unwrap_or_default()))
}

/// The editor of `pane`'s tab of `path`, if it has one.
fn landed_editor(pane: &Entity<Pane>, path: &Path, cx: &App) -> Option<Entity<EditorView>> {
    let pane = pane.read(cx);
    let tab = pane.position(path)?;
    pane.tabs()[tab].editor().cloned()
}

/// Says `text` as `editor`'s vim message, or with no editor or vim off as
/// a toast.
fn say(editor: Option<&Entity<EditorView>>, text: impl Into<String>, cx: &mut App) {
    let text = text.into();
    match editor {
        Some(editor) if editor.read(cx).vim_state().is_some() => {
            editor.update(cx, |editor, cx| editor.set_vim_message(text, cx));
        }
        _ => toast::show(cx, Toast::info(text)),
    }
}
