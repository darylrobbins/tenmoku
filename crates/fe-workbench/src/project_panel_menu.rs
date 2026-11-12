//! The project tree's right-click menu, as VS Code's explorer and
//! JetBrains' project view offer one. Its items are the panel's own
//! `project_panel.*` commands, each run through [`ProjectPanel::run_op`] as
//! its key runs it, titled by the command's title and showing its key
//! right-aligned, read from the keymap as the menu is drawn.
//!
//! A right click on a row selects it (it doesn't open a file or toggle a
//! folder, as a left click does) and offers, on a folder: New File, New
//! Folder | Rename, Duplicate, Move to Trash | Cut, Copy, Paste | Copy Path,
//! Copy Relative Path | Collapse All Folders. A file's menu is the same
//! without New File and New Folder; its Paste goes beside it, as the key's
//! does. A right click on the empty space below the rows selects nothing
//! and offers New File, New Folder | Paste | Refresh, each acting on the
//! root. Items that would do nothing now are left out, not disabled: Paste
//! with nothing cut or copied, or with a cut entry that would land where it
//! is.
//!
//! The menu is drawn where it was asked for, over the window. A click
//! outside it, Escape, focus leaving the panel, a refresh or its row going
//! closes it; Up and Down move its highlight, wrapping, and Enter chooses
//! it. While it's open the panel's key context is [`MENU_KEY_CONTEXT`], so
//! the tree's own keys (vim's included) wait. A right click while a name
//! field is open does nothing: the field keeps focus and commits nothing.

use fe_command::{ContextFrame, ContextStack};
use fe_project::{EntryKind, NodeId};
use fe_ui::{MenuEntry, MenuPanel, Role};
use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, Point, SharedString, Subscription, Window, anchored, deferred, div,
};
use kiln_l10n::{tr, verbatim};

use super::{KEY_CONTEXT, PanelOp, ProjectPanel};

/// The panel's key context while its menu is open: no binding names it,
/// so Escape, Up, Down and Enter reach the menu.
pub(super) const MENU_KEY_CONTEXT: &str = verbatim!("project_panel_menu");

/// An item of the menu: a panel command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuItem {
    NewFile,
    NewFolder,
    Rename,
    Duplicate,
    Delete,
    Cut,
    Copy,
    Paste,
    CopyPath,
    CopyRelativePath,
    CollapseAll,
    Refresh,
}

use MenuItem::*;

/// A folder row's menu; `None` is a separator.
const FOLDER_ITEMS: &[Option<MenuItem>] = &[
    Some(NewFile),
    Some(NewFolder),
    None,
    Some(Rename),
    Some(Duplicate),
    Some(Delete),
    None,
    Some(Cut),
    Some(Copy),
    Some(Paste),
    None,
    Some(CopyPath),
    Some(CopyRelativePath),
    None,
    Some(CollapseAll),
];

/// The empty space's menu, below the rows.
const EMPTY_ITEMS: &[Option<MenuItem>] = &[
    Some(NewFile),
    Some(NewFolder),
    None,
    Some(Paste),
    None,
    Some(Refresh),
];

impl MenuItem {
    /// The op it runs.
    fn op(self) -> PanelOp {
        match self {
            NewFile => PanelOp::NewFile,
            NewFolder => PanelOp::NewFolder,
            Rename => PanelOp::Rename,
            Duplicate => PanelOp::Duplicate,
            Delete => PanelOp::Delete,
            Cut => PanelOp::Cut,
            Copy => PanelOp::Copy,
            Paste => PanelOp::Paste,
            CopyPath => PanelOp::CopyPath,
            CopyRelativePath => PanelOp::CopyRelativePath,
            CollapseAll => PanelOp::CollapseAll,
            Refresh => PanelOp::Refresh,
        }
    }

    /// The command it is, which titles it and finds its key.
    pub(crate) fn command(self) -> &'static str {
        match self {
            NewFile => "project_panel.new_file",
            NewFolder => "project_panel.new_folder",
            Rename => "project_panel.rename",
            Duplicate => "project_panel.duplicate",
            Delete => "project_panel.delete",
            Cut => "project_panel.cut",
            Copy => "project_panel.copy",
            Paste => "project_panel.paste",
            CopyPath => "project_panel.copy_path",
            CopyRelativePath => "project_panel.copy_relative_path",
            CollapseAll => "project_panel.collapse_all",
            Refresh => "project_panel.refresh",
        }
    }

    /// Its title: its command's, in the active locale.
    pub(crate) fn title(self) -> SharedString {
        fe_command::l10n::command_label(self.command(), "")
    }
}

/// The open menu.
pub(super) struct TreeMenu {
    /// The row it's for; `None` for the empty space.
    target: Option<NodeId>,
    /// Its items; `None` is a separator.
    entries: Vec<Option<MenuItem>>,
    /// The entry Up and Down moved to, or the pointer is over, which Enter
    /// chooses: none until either does.
    highlighted: Option<usize>,
    /// Where it was asked for, in the window.
    position: Point<Pixels>,
    /// Closes it when focus leaves the panel.
    _focus_out: Subscription,
}

/// What the menu draws for `entries`: each item's title and the key that
/// runs it where the tree has focus, as the tree has keys while the menu
/// isn't open.
fn menu_entries(entries: &[Option<MenuItem>], cx: &App) -> Vec<MenuEntry> {
    let mut frame = ContextFrame::new(KEY_CONTEXT);
    if fe_gpui::vim_enabled(cx) {
        frame = frame.flag("vim");
    }
    let stack: ContextStack = [ContextFrame::new(crate::KEY_CONTEXT), frame]
        .into_iter()
        .collect();
    (entries.iter())
        .map(|entry| match entry {
            Some(item) => MenuEntry::Item {
                title: item.title(),
                shortcut: (crate::pane_toolbar::menu_shortcut(item.command(), &stack, cx))
                    .map(Into::into),
            },
            None => MenuEntry::Separator,
        })
        .collect()
}

/// `items` without those `keep` leaves out, and without a separator
/// that would start or end the menu or follow another.
fn kept(items: &[Option<MenuItem>], keep: impl Fn(MenuItem) -> bool) -> Vec<Option<MenuItem>> {
    let mut entries: Vec<Option<MenuItem>> = Vec::new();
    for entry in items.iter().copied() {
        match entry {
            Some(item) if keep(item) => entries.push(entry),
            Some(_) => {}
            None if entries.last().is_some_and(Option::is_some) => entries.push(None),
            None => {}
        }
    }
    if entries.last() == Some(&None) {
        entries.pop();
    }
    entries
}

impl ProjectPanel {
    /// A right click at `position` on row `id`, or with `None` on the
    /// empty space below the rows: selects it (or nothing) and opens its
    /// menu. Nothing while a name field is open.
    pub(super) fn open_menu(
        &mut self,
        id: Option<NodeId>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.edit.is_some() {
            return;
        }
        let Some(tree) = self.tree() else {
            return;
        };
        let items = match id {
            Some(id) => match tree.row(id).map(|row| row.kind) {
                Some(EntryKind::Dir) => FOLDER_ITEMS,
                Some(_) => &FOLDER_ITEMS[3..],
                None => return,
            },
            None => EMPTY_ITEMS,
        };
        self.cancel_reveal();
        self.selected = id;
        if let Some(ix) = self.selected_index() {
            self.selected_hint = ix;
        }
        let can_paste = self.can_paste();
        let entries = kept(items, |item| item != Paste || can_paste);
        window.focus(&self.focus, cx);
        let focus_out = cx.on_focus_out(&self.focus, window, |panel, _, _, cx| {
            panel.close_menu(cx);
        });
        self.menu = Some(TreeMenu {
            target: id,
            entries,
            highlighted: None,
            position,
            _focus_out: focus_out,
        });
        cx.notify();
    }

    /// Closes the open menu, if there's one.
    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Closes the open menu if the row it's for is gone from the tree.
    pub(super) fn close_gone_menu(&mut self, cx: &mut Context<Self>) {
        let target = self.menu.as_ref().and_then(|menu| menu.target);
        let gone = target.is_some_and(|id| self.tree().is_none_or(|tree| tree.row(id).is_none()));
        if gone {
            self.close_menu(cx);
        }
    }

    /// Escape, Up, Down and Enter while the menu is open: see the module
    /// docs. Other keys do nothing then.
    pub(super) fn on_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.is_none() {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.close_menu(cx),
            "up" | "down" => self.move_menu_highlight(keystroke.key == "down", cx),
            "enter" => self.choose_menu_highlight(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Moves the open menu's highlight to the next item, or with `down`
    /// false the one before, wrapping and passing separators over; from
    /// none, the first or the last.
    fn move_menu_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let items: Vec<usize> = (menu.entries.iter().enumerate())
            .filter_map(|(ix, entry)| entry.map(|_| ix))
            .collect();
        let Some(last) = items.len().checked_sub(1) else {
            return;
        };
        let at = menu
            .highlighted
            .and_then(|ix| items.iter().position(|&item| item == ix));
        let next = match (at, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(at), true) if at >= last => 0,
            (Some(at), true) => at + 1,
            (Some(at), false) => at.checked_sub(1).unwrap_or(last),
        };
        menu.highlighted = Some(items[next]);
        cx.notify();
    }

    /// Enter in the open menu: runs the highlighted item, if any, as a
    /// click on it does.
    fn choose_menu_highlight(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let menu = self.menu.as_ref();
        let item = menu.and_then(|menu| *menu.entries.get(menu.highlighted?)?);
        if let Some(item) = item {
            self.run_menu_item(item, window, cx);
        }
    }

    /// Closes the menu and runs `item` on its row (the root for the empty
    /// space's), as the item's key would.
    pub(super) fn run_menu_item(
        &mut self,
        item: MenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        cx.notify();
        self.selected = menu.target;
        self.run_op(item.op(), window, cx);
    }

    /// The open menu, drawn over the window where it was asked for.
    pub(super) fn render_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let entries = menu_entries(&menu.entries, cx);
        let items = menu.entries.clone();
        let panel = cx.entity().downgrade();
        let hovered = panel.clone();
        let ui = fe_ui::ui_theme(cx);
        let menu_panel = MenuPanel::new(entries, &ui)
            .highlighted(menu.highlighted)
            .on_click(move |ix, window, cx| {
                let Some(Some(item)) = items.get(ix).copied() else {
                    return;
                };
                (panel.update(cx, |panel, cx| panel.run_menu_item(item, window, cx))).ok();
            })
            .on_hover(move |ix, _, cx| {
                (hovered.update(cx, |panel, cx| {
                    if let Some(menu) = &mut panel.menu
                        && menu.highlighted != Some(ix)
                        && menu.entries.get(ix).is_some_and(Option::is_some)
                    {
                        menu.highlighted = Some(ix);
                        cx.notify();
                    }
                }))
                .ok();
            });
        let wrapper = div()
            .id("project-tree-menu")
            .debug_selector(|| "project-tree-menu".into())
            .on_mouse_down_out(cx.listener(|panel, _, _, cx| panel.close_menu(cx)))
            .child(menu_panel);
        let wrapper = fe_ui::node(
            wrapper,
            window,
            cx,
            Role::Menu,
            tr!("workbench-project-menu"),
        );
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window()
                    .child(wrapper),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
impl ProjectPanel {
    /// The open menu's entries by command, `None` for a separator.
    pub(crate) fn menu_commands(&self) -> Option<Vec<Option<&'static str>>> {
        let menu = self.menu.as_ref()?;
        Some(
            menu.entries
                .iter()
                .map(|entry| entry.map(MenuItem::command))
                .collect(),
        )
    }

    /// The open menu's items as drawn: each title and its key, if any.
    pub(crate) fn menu_drawn(&self, cx: &App) -> Option<Vec<(String, Option<String>)>> {
        let menu = self.menu.as_ref()?;
        let drawn = menu_entries(&menu.entries, cx)
            .into_iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item { title, shortcut } => {
                    Some((title.to_string(), shortcut.map(|keys| keys.to_string())))
                }
                MenuEntry::Separator => None,
            });
        Some(drawn.collect())
    }

    /// The open menu's highlighted item's command, if one is.
    pub(crate) fn menu_highlighted(&self) -> Option<&'static str> {
        let menu = self.menu.as_ref()?;
        Some((*menu.entries.get(menu.highlighted?)?)?.command())
    }
}
