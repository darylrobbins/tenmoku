//! The project tree's New menu, which the `+` at the left of the dock's
//! header opens under itself ([`super::pane_actions`]): File and Folder,
//! as the `project_panel.new_file` and `project_panel.new_folder` commands
//! do, then typed files, a language's first if the root has its project
//! marker, each with its template ([`super::templates`]).
//!
//! Each entry names its new entry in the inline name field, in the folder
//! New File uses: the selected folder, the selected file's folder, or the
//! root. A typed entry's field is prefilled with `Untitled.<ext>`, its stem
//! selected ([`ProjectPanel::start_typed_file`]); the file opens once
//! made, as New File's does.
//!
//! The menu takes focus, as the right-click menu does ([`super::menu`]),
//! and the panel's key context is that menu's while it's open: Up and Down
//! move its highlight, wrapping, Enter chooses it, and Escape closes it,
//! giving focus back to what had it outside the panel. A click outside it
//! (but on the `+`, which closes it), focus leaving the panel or the tree
//! being opened again close it too.

use fe_ui::{MenuEntry, MenuPanel, Role};
use gpui::{
    Anchor, AnyElement, Bounds, Context, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseDownEvent, ParentElement as _, Pixels, SharedString, Subscription, Window,
    anchored, deferred, div, point, px,
};
use kiln_l10n::tr;

use super::templates::{self, FileType};
use super::{PanelOp, ProjectPanel, State, menu};

/// The command the empty panel's button and the `+` with no folder run.
pub(super) const OPEN_FOLDER: &str = "workbench.open_folder";

/// An entry of the New menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NewItem {
    File,
    Folder,
    Typed(FileType),
    /// With no folder open: the one way to start a project.
    OpenFolder,
}

impl NewItem {
    /// Its title, in the active locale.
    pub(crate) fn title(self) -> SharedString {
        match self {
            NewItem::File => tr!("workbench-project-new-file"),
            NewItem::Folder => tr!("workbench-project-new-folder"),
            NewItem::Typed(file_type) => file_type.title(),
            NewItem::OpenFolder => tr!("workbench-project-open-folder"),
        }
    }

    /// The command it does, whose key it shows, if it's one.
    fn command(self) -> Option<&'static str> {
        match self {
            NewItem::File => Some(menu::MenuItem::NewFile.command()),
            NewItem::Folder => Some(menu::MenuItem::NewFolder.command()),
            NewItem::Typed(_) => None,
            NewItem::OpenFolder => Some(OPEN_FOLDER),
        }
    }
}

/// The open New menu.
pub(super) struct NewMenu {
    /// Its entries; `None` is a separator.
    entries: Vec<Option<NewItem>>,
    /// The entry Up and Down moved to, or the pointer is over, which Enter
    /// chooses: none until either does.
    highlighted: Option<usize>,
    /// The `+`'s bounds, in the window, which it's drawn under.
    anchor: Bounds<Pixels>,
    /// What had focus outside the panel as it opened, which Escape gives
    /// focus back to.
    back: Option<FocusHandle>,
    /// Closes it when focus leaves the panel.
    _focus_out: Subscription,
}

impl ProjectPanel {
    /// The `+`'s click, its bounds `anchor`: opens the New menu under it,
    /// or closes it if it's open.
    pub(super) fn toggle_new_menu(
        &mut self,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.new_menu.is_some() {
            self.dismiss_new_menu(window, cx);
        } else {
            self.open_new_menu(anchor, window, cx);
        }
    }

    /// Opens the New menu under the `+` as last drawn, or toggles it shut
    /// if it's open. With no `+` drawn it hangs from the window's top left.
    fn open_new_menu_from_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = kiln_shell::dock::drawn_action_bounds(window, super::NEW_BUTTON, cx)
            .unwrap_or_else(|| Bounds::new(point(px(8.), px(8.)), gpui::size(px(0.), px(24.))));
        self.toggle_new_menu(anchor, window, cx);
    }

    /// Has the next frames open the New menu under the `+`, once the dock's
    /// header has drawn it: see [`Self::draw_pending_new_menu`].
    pub(super) fn open_new_menu_when_drawn(&mut self, cx: &mut Context<Self>) {
        self.pending_new_menu = 2;
        cx.notify();
    }

    /// Called as the panel renders: counts a wait for the `+` down, a frame
    /// each, then opens the menu from the render's end.
    pub(super) fn draw_pending_new_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.pending_new_menu {
            0 => {}
            1 => {
                self.pending_new_menu = 0;
                let panel = cx.entity().downgrade();
                window.defer(cx, move |window, cx| {
                    (panel.update(cx, |panel, cx| panel.open_new_menu_from_key(window, cx))).ok();
                });
            }
            _ => {
                self.pending_new_menu -= 1;
                cx.notify();
            }
        }
    }

    /// Opens the New menu under `anchor`, focused; nothing with no tree.
    /// A name field open meanwhile closes, naming nothing, as does the
    /// right-click menu; nothing while its operation runs or a rename
    /// waits for language servers, as the field keeps focus then.
    pub(super) fn open_new_menu(
        &mut self,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.edit_busy() {
            return;
        }
        let Some(tree) = self.tree() else {
            if matches!(self.state, State::NoFolder) {
                self.open_no_folder_menu(anchor, window, cx);
            }
            return;
        };
        let root = tree.root();
        let at_root = |name: &str| {
            (tree.children(root).into_iter().flatten())
                .filter_map(|&id| tree.row(id))
                .any(|row| row.name.to_str() == Some(name))
        };
        let typed = templates::typed_entries(at_root);
        let entries = [Some(NewItem::File), Some(NewItem::Folder), None]
            .into_iter()
            .chain(
                typed
                    .into_iter()
                    .map(|file_type| Some(NewItem::Typed(file_type))),
            )
            .collect();
        let back = (window.focused(cx)).filter(|_| !self.focus.contains_focused(window, cx));
        self.menu = None;
        self.edit = None;
        window.focus(&self.focus, cx);
        let focus_out = cx.on_focus_out(&self.focus, window, |panel, _, _, cx| {
            panel.close_menu(cx);
        });
        self.new_menu = Some(NewMenu {
            entries,
            highlighted: None,
            anchor,
            back,
            _focus_out: focus_out,
        });
        cx.notify();
    }

    /// With no folder open the `+` offers the one way to start: Open Folder….
    fn open_no_folder_menu(
        &mut self,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back = (window.focused(cx)).filter(|_| !self.focus.contains_focused(window, cx));
        self.menu = None;
        window.focus(&self.focus, cx);
        let focus_out = cx.on_focus_out(&self.focus, window, |panel, _, _, cx| {
            panel.close_menu(cx);
        });
        self.new_menu = Some(NewMenu {
            entries: vec![Some(NewItem::OpenFolder)],
            highlighted: None,
            anchor,
            back,
            _focus_out: focus_out,
        });
        cx.notify();
    }

    /// Runs `workbench.open_folder` from the panel's focus, once this event
    /// is over.
    pub(super) fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The button's own focus is outside the panel's key context.
        window.focus(&self.focus, cx);
        window.defer(cx, |window, cx| {
            let Ok(invocation) = fe_command::Invocation::of(OPEN_FOLDER) else {
                return;
            };
            let stack = kiln::platform::keys::context_stack(&window.context_stack(), cx);
            if let Err(error) = kiln::platform::command_actions::dispatch_invocation(
                invocation.context(stack),
                window,
                cx,
            ) {
                log::warn!("{error}");
            }
        });
    }

    /// Closes the New menu, choosing nothing, and gives focus back to
    /// what had it before it opened, if anything outside the panel did.
    fn dismiss_new_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.new_menu.take() else {
            return;
        };
        if let Some(back) = menu.back {
            window.focus(&back, cx);
        }
        cx.notify();
    }

    /// Escape, Up, Down and Enter while the New menu is open: see the
    /// module docs. Other keys do nothing then.
    pub(super) fn on_new_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = &mut self.new_menu else {
            return;
        };
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.dismiss_new_menu(window, cx),
            "up" | "down" => {
                let items: Vec<bool> = open.entries.iter().map(Option::is_some).collect();
                let down = keystroke.key == "down";
                open.highlighted = menu::next_highlight(&items, open.highlighted, down);
                cx.notify();
            }
            "enter" => {
                let item = open.highlighted.and_then(|ix| *open.entries.get(ix)?);
                if let Some(item) = item {
                    self.choose_new(item, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Closes the New menu and names `item`'s new entry in the name field.
    pub(super) fn choose_new(
        &mut self,
        item: NewItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.new_menu.take().is_none() {
            return;
        }
        cx.notify();
        match item {
            NewItem::OpenFolder => self.open_folder(window, cx),
            NewItem::File => self.run_op(PanelOp::NewFile, window, cx),
            NewItem::Folder => self.run_op(PanelOp::NewFolder, window, cx),
            NewItem::Typed(file_type) => {
                // As `run_op` does before New File.
                self.cancel_reveal();
                self.close_menu(cx);
                self.start_typed_file(file_type, window, cx);
            }
        }
    }

    /// What the New menu draws for its entries: each title, and File's and
    /// Folder's keys where the tree has focus.
    fn new_menu_entries(entries: &[Option<NewItem>], cx: &gpui::App) -> Vec<MenuEntry> {
        let stack = menu::tree_context(cx);
        (entries.iter())
            .map(|entry| match entry {
                Some(item) => MenuEntry::Item {
                    title: item.title(),
                    shortcut: (item.command())
                        .and_then(|command| crate::pane_toolbar::menu_shortcut(command, &stack, cx))
                        .map(Into::into),
                },
                None => MenuEntry::Separator,
            })
            .collect()
    }

    /// The open New menu, drawn over the window under the `+`, kept inside
    /// the window.
    pub(super) fn render_new_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.new_menu.as_ref()?;
        let entries = Self::new_menu_entries(&open.entries, cx);
        let items = open.entries.clone();
        let panel = cx.entity().downgrade();
        let hovered = panel.clone();
        let ui = fe_ui::ui_theme(cx);
        let menu_panel = MenuPanel::new(entries, &ui)
            .highlighted(open.highlighted)
            .on_click(move |ix, window, cx| {
                let Some(Some(item)) = items.get(ix).copied() else {
                    return;
                };
                (panel.update(cx, |panel, cx| panel.choose_new(item, window, cx))).ok();
            })
            .on_hover(move |ix, _, cx| {
                (hovered.update(cx, |panel, cx| {
                    if let Some(open) = &mut panel.new_menu
                        && open.highlighted != Some(ix)
                        && open.entries.get(ix).is_some_and(Option::is_some)
                    {
                        open.highlighted = Some(ix);
                        cx.notify();
                    }
                }))
                .ok();
            });
        // A press on the `+` is its click's, which closes the menu.
        let anchor = open.anchor;
        let wrapper = div()
            .id("project-new-menu")
            .debug_selector(|| "project-new-menu".into())
            .on_mouse_down_out(cx.listener(move |panel, event: &MouseDownEvent, _, cx| {
                if !anchor.contains(&event.position) {
                    panel.close_menu(cx);
                }
            }))
            .child(menu_panel);
        let wrapper = fe_ui::node(
            wrapper,
            window,
            cx,
            Role::Menu,
            tr!("workbench-project-new-menu"),
        );
        let below = anchor.bottom_left() + point(px(0.), px(2.));
        Some(
            deferred(
                anchored()
                    .position(below)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(4.))
                    .child(wrapper),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
impl ProjectPanel {
    /// The open New menu's entries, `None` for a separator.
    pub(crate) fn new_menu_items(&self) -> Option<Vec<Option<NewItem>>> {
        Some(self.new_menu.as_ref()?.entries.clone())
    }

    /// The open New menu's items as drawn: each title and its key, if any.
    pub(crate) fn new_menu_drawn(&self, cx: &gpui::App) -> Option<Vec<(String, Option<String>)>> {
        let open = self.new_menu.as_ref()?;
        let drawn =
            (Self::new_menu_entries(&open.entries, cx).into_iter()).filter_map(
                |entry| match entry {
                    MenuEntry::Item { title, shortcut } => {
                        Some((title.to_string(), shortcut.map(|keys| keys.to_string())))
                    }
                    MenuEntry::Separator => None,
                },
            );
        Some(drawn.collect())
    }

    /// The open New menu's highlighted entry, if one is.
    pub(crate) fn new_menu_highlighted(&self) -> Option<NewItem> {
        let open = self.new_menu.as_ref()?;
        *open.entries.get(open.highlighted?)?
    }

    /// The `+`'s bounds the open New menu is drawn under.
    pub(crate) fn new_menu_anchor(&self) -> Option<Bounds<Pixels>> {
        Some(self.new_menu.as_ref()?.anchor)
    }
}

impl crate::Workbench {
    /// `project_panel.new_menu`, from the keyboard or the palette, wherever
    /// focus is: shows the Project dock, focused, and opens the New menu
    /// under its `+`; unless the tree has focus, once its header has been drawn
    /// ([`ProjectPanel::open_new_menu_when_drawn`]).
    pub(crate) fn show_new_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Only a tree that already has focus is known to be drawn, its `+` too.
        let drawn = self.panel().read(cx).focus.contains_focused(window, cx);
        self.run_dock_op(crate::dock::DockOp::ShowProject, window, cx);
        self.panel().update(cx, |panel, cx| match drawn {
            true => panel.open_new_menu_from_key(window, cx),
            false => panel.open_new_menu_when_drawn(cx),
        });
    }
}
