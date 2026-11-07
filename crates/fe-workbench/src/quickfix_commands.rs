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
//! `:grep`, `:vimgrep` and `:make` (and their `:l` twins) fill lists
//! (`quickfix_fill.rs`), through [`Workbench::push_quickfix_list_to`].
//!
//! `:copen` and `:cwindow` (and the typed `quickfix.show`) show the
//! quickfix list in the sidebar's Quickfix List panel, focused
//! (`quickfix_panel.rs`); `:lopen` and `:lwindow` (`location.show`) show the
//! active pane's location list there, or say `E776: No location list`.
//! `:cclose` hides the panel if it shows the quickfix list, `:lclose` if it
//! shows the active pane's location list; `quickfix.toggle` and
//! `location.toggle` hide it if it shows that list, else show it. The
//! panel hears of every change to the list it shows
//! ([`Workbench::quickfix_changed`]), and a row picked there lands as
//! `:cc N` does ([`Workbench::open_quickfix_row`]).

use std::{collections::HashMap, path::Path};

use fe_gpui::EditorView;
use fe_vim::{QuickfixAction, QuickfixCommand};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Focusable as _, Global, Window};
use kiln_shell::WorkspaceEvent;

use crate::{
    DockPanel, Pane, Workbench,
    dock::{DockOp, QuickfixPanel},
    quickfix::{QuickfixError, QuickfixItem, QuickfixList, QuickfixStack, Quickfixes},
    workbench::toast::{self, Toast},
};

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
            cx.subscribe(self.workspace(), move |this, _, event, cx| {
                if let WorkspaceEvent::PaneRemoved(pane) = event
                    && let Some(lists) = cx.default_global::<Lists>().0.get_mut(&id)
                {
                    lists.drop_pane(pane);
                    this.quickfix_changed(cx);
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
    /// the active pane's location stack, as
    /// [`Workbench::push_quickfix_list_to`] does. For the tests.
    #[cfg(test)]
    pub(crate) fn push_quickfix_list(
        &mut self,
        location: bool,
        list: QuickfixList,
        cx: &mut Context<Self>,
    ) {
        let pane = location.then(|| self.pane().entity_id());
        self.push_quickfix_list_to(pane, list, cx);
    }

    /// Puts `list` on this window's quickfix stack, or with a `pane` on that
    /// pane's location stack, after the list in use (dropping any newer,
    /// and past ten the oldest), and uses it, the item it selects selected.
    /// Nothing is opened. False, and nothing kept, if `pane` is no longer
    /// one of this window's.
    pub(crate) fn push_quickfix_list_to(
        &mut self,
        pane: Option<EntityId>,
        list: QuickfixList,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(pane) = pane {
            let panes = self.pane_group(cx).panes();
            if !panes.iter().any(|open| open.entity_id() == pane) {
                return false;
            }
        }
        let lists = self.quickfixes(cx);
        match pane {
            Some(pane) => lists.push_location(pane, list),
            None => lists.quickfix.push(list),
        }
        self.quickfix_changed(cx);
        true
    }

    /// The summary line of the list in use (`error list 1 of 2; 3 errors
    /// …`): the quickfix stack's, or with a `pane` that pane's location
    /// stack's.
    pub(crate) fn quickfix_summary(
        &mut self,
        pane: Option<EntityId>,
        cx: &mut Context<Self>,
    ) -> String {
        let lists = self.quickfixes(cx);
        let stack = match pane {
            Some(pane) => lists.location(&pane).ok(),
            None => Some(&mut lists.quickfix),
        };
        stack.map(|stack| stack.summary()).unwrap_or_default()
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
        let action = match action {
            QuickfixAction::Grep(args) => return self.grep(&args, location, editor, window, cx),
            QuickfixAction::VimGrep(args) => {
                return self.vimgrep(&args, location, editor, window, cx);
            }
            QuickfixAction::Make(args) => return self.make(&args, location, editor, window, cx),
            QuickfixAction::Open => return self.show_quickfix_panel(location, editor, window, cx),
            QuickfixAction::Close => return self.close_quickfix_panel(location, window, cx),
            action => action,
        };
        let pane = self.pane().entity_id();
        let root = self.root().map(Path::to_owned);
        let lists = self.quickfixes(cx);
        let outcome = lists
            .stack(location, &pane)
            .and_then(|stack| answer(stack, action, root.as_deref()));
        if outcome.is_ok() {
            self.quickfix_changed(cx);
        }
        match outcome {
            Err(error) => say(editor, error.to_string(), cx),
            Ok(Outcome::Say(text)) => say(editor, text, cx),
            Ok(Outcome::Land(item, text)) => self.land_on(item, text, window, cx),
        }
    }

    /// Tells the Quickfix List panel, if it has been made, that a list has
    /// changed, or moved to another item: it redraws, its cursor on the
    /// current item.
    pub(crate) fn quickfix_changed(&self, cx: &mut App) {
        if let Some(panel) = self.dock().read(cx).quickfix().cloned() {
            panel.update(cx, QuickfixPanel::list_changed);
        }
    }

    /// The Quickfix List panel, made and registered on first use.
    fn quickfix_panel(&mut self, cx: &mut Context<Self>) -> Entity<QuickfixPanel> {
        if let Some(panel) = self.dock().read(cx).quickfix() {
            return panel.clone();
        }
        let workbench = cx.entity();
        let theme = self.pane().read(cx).theme().clone();
        let panel = cx.new(|cx| QuickfixPanel::new(&workbench, &theme, cx));
        (self.dock()).update(cx, |dock, cx| dock.set_quickfix(panel.clone(), cx));
        panel
    }

    /// Whether the panel shows: the dock visible with it, and it showing
    /// the quickfix list, or with `location` the active pane's location
    /// list.
    fn quickfix_panel_shown(&self, location: bool, cx: &App) -> bool {
        let dock = self.dock().read(cx);
        let Some(panel) = dock.quickfix() else {
            return false;
        };
        let source = location.then(|| self.pane().entity_id());
        dock.is_visible()
            && dock.active() == DockPanel::Quickfix
            && panel.read(cx).source() == source
    }

    /// `:copen`, or with `location` `:lopen`: shows the panel, focused,
    /// with the quickfix list or the active pane's location list; for a
    /// pane with none, `E776: No location list` in `editor` (or the active
    /// editor), and nothing shown.
    pub(crate) fn show_quickfix_panel(
        &mut self,
        location: bool,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane().entity_id();
        if location && self.quickfixes(cx).location(&pane).is_err() {
            let editor = editor.cloned().or_else(|| self.active_editor(cx).cloned());
            return say(
                editor.as_ref(),
                QuickfixError::NoLocationList.to_string(),
                cx,
            );
        }
        // Set up to forget the pane's list with it.
        self.quickfixes(cx);
        let panel = self.quickfix_panel(cx);
        panel.update(cx, |panel, cx| {
            panel.show_list(location.then_some(pane), cx)
        });
        self.run_dock_op(DockOp::ShowQuickfix, window, cx);
        window.focus(&panel.focus_handle(cx), cx);
    }

    /// `:cclose`, or with `location` `:lclose`: hides the panel if it shows
    /// that list, focus going back to the active editor.
    pub(crate) fn close_quickfix_panel(
        &mut self,
        location: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quickfix_panel_shown(location, cx) {
            self.run_dock_op(DockOp::Toggle, window, cx);
        }
    }

    /// `quickfix.toggle`, or with `location` `location.toggle`: hides the
    /// panel if it shows that list, else shows it as `:copen` does.
    pub(crate) fn toggle_quickfix_panel(
        &mut self,
        location: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quickfix_panel_shown(location, cx) {
            self.close_quickfix_panel(location, window, cx);
        } else {
            self.show_quickfix_panel(location, None, window, cx);
        }
    }

    /// A row picked in the panel: lands on item `ix` (from 0) of the
    /// quickfix list, or with a `pane` that pane's location list, as `:cc
    /// N` does, making it current; a location list's item opens in its
    /// pane, made active. Nothing if the list or pane has gone.
    pub(crate) fn open_quickfix_row(
        &mut self,
        pane: Option<EntityId>,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lists = self.quickfixes(cx);
        let stack = match pane {
            Some(pane) => lists.location(&pane).ok(),
            None => Some(&mut lists.quickfix),
        };
        let Some(list) = stack.and_then(|stack| stack.current_mut().ok()) else {
            return;
        };
        let number = u32::try_from(ix.saturating_add(1)).unwrap_or(u32::MAX);
        let Ok(item) = list.go(Some(number)).cloned() else {
            return;
        };
        let text = list.landed_message();
        if let Some(pane) = pane {
            let Some(pane) = self
                .panes(cx)
                .into_iter()
                .find(|open| open.entity_id() == pane)
            else {
                return;
            };
            if pane != *self.pane() {
                self.activate(pane, window, cx);
            }
        }
        self.quickfix_changed(cx);
        self.land_on(item, text, window, cx);
        let focus = self.pane().read(cx).focus_target(cx);
        window.focus(&focus, cx);
    }

    /// Opens `item`'s file in the active pane at its row and column, then
    /// says `text` in the editor there.
    pub(crate) fn land_on(
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
        Grep(_) | VimGrep(_) | Make(_) | Open | Close => {
            unreachable!("run_quickfix fills lists and shows them itself")
        }
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

/// The list in use that `workbench`'s Quickfix List panel shows: its
/// quickfix stack's, or with a `pane` that pane's location stack's. None
/// before the window has had a list, or for a pane with no location list.
pub(crate) fn shown_list(
    workbench: EntityId,
    pane: Option<EntityId>,
    cx: &mut App,
) -> Option<&QuickfixList> {
    let lists = cx.default_global::<Lists>().0.get_mut(&workbench)?;
    let stack = match pane {
        Some(pane) => lists.location(&pane).ok()?,
        None => &mut lists.quickfix,
    };
    stack.current()
}

/// Says `text` as `editor`'s vim message, or with no editor or vim off as
/// a toast.
pub(crate) fn say(editor: Option<&Entity<EditorView>>, text: impl Into<String>, cx: &mut App) {
    let text = text.into();
    match editor {
        Some(editor) if editor.read(cx).vim_state().is_some() => {
            editor.update(cx, |editor, cx| editor.set_vim_message(text, cx));
        }
        _ => toast::show(cx, Toast::info(text)),
    }
}
