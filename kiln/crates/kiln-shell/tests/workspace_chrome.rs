// SPDX-License-Identifier: Apache-2.0

//! The workspace's chrome: docks added by position, their rail tooltips
//! finding the show key, focus leaving a hidden panel, and the built-in
//! status bar following the active item and the workspace's own items.

use std::{path::Path, time::Duration};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Modifiers, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};
use kiln_core::{Keymap, KeymapLayer, Os};
use kiln_platform::{Capabilities, keys};
use kiln_shell::{
    DockItem, DockPosition, DockView as _, ItemHandle, PaneItem, PaneView, PanelId, StatusItem,
    StatusSide, Workspace,
};
use kiln_ui::Icon;

const APP: &str = "chrome_app";

/// A dock panel: its id, show command and focus.
struct Panel {
    id: &'static str,
    show: Option<&'static str>,
    focus: FocusHandle,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full().child(self.id)
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        PanelId(self.id)
    }

    fn title(&self, _: &App) -> SharedString {
        format!("The {}", self.id).into()
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::Files
    }

    fn show_command(&self) -> Option<&'static str> {
        self.show
    }
}

/// An item with status items of its own.
struct Page {
    title: &'static str,
    status: Vec<StatusItem>,
    focus: FocusHandle,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Page {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Page {
    const KIND: &'static str = "test.chrome_page";

    fn tab_title(&self, _: &App) -> SharedString {
        self.title.into()
    }

    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        self.status.clone()
    }
}

struct Opened<'a> {
    workspace: Entity<Workspace>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn panel(&mut self, id: &'static str, show: Option<&'static str>) -> Entity<Panel> {
        self.cx.new(|cx| Panel {
            id,
            show,
            focus: cx.focus_handle(),
        })
    }

    fn add_panel(&mut self, position: DockPosition, panel: &Entity<Panel>) {
        let (workspace, panel) = (self.workspace.clone(), panel.clone());
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.add_panel(position, panel, window, cx)
            })
        });
        self.draw();
    }

    fn add_page(&mut self, title: &'static str, status: Vec<StatusItem>) -> Entity<Page> {
        let page = self.cx.new(|cx| Page {
            title,
            status,
            focus: cx.focus_handle(),
        });
        let (workspace, item) = (self.workspace.clone(), Box::new(page.clone()));
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.add_item(item, window, cx))
        });
        self.draw();
        page
    }

    fn dock(&mut self, position: DockPosition) -> Entity<kiln_shell::Dock> {
        (self.workspace).read_with(self.cx, |workspace, _| {
            workspace.dock(position).cloned().unwrap()
        })
    }

    fn visible(&mut self, position: DockPosition) -> bool {
        let dock = self.dock(position);
        dock.read_with(self.cx, |dock, _| dock.is_visible())
    }

    /// Whether the dock at `position` is drawn.
    fn drawn(&mut self, position: DockPosition) -> bool {
        let dock = self.dock(position);
        dock.read_with(self.cx, |dock, _| {
            dock.dock().drawn().column_bounds().is_some()
        })
    }

    fn toggle(&mut self, position: DockPosition) {
        let workspace = self.workspace.clone();
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_dock(position, window, cx)
            })
        });
        self.draw();
    }

    fn focus(&mut self, focus: &FocusHandle) {
        self.cx.update(|window, cx| window.focus(focus, cx));
        self.draw();
    }

    fn focused(&mut self, focus: &FocusHandle) -> bool {
        self.cx.update(|window, _| focus.is_focused(window))
    }

    fn status_width(&mut self, id: &'static str) -> Option<f32> {
        let bounds = self
            .cx
            .debug_bounds(Box::leak(format!("status-item:{id}").into_boxed_str()))?;
        Some(bounds.size.width.into())
    }
}

fn open(cx: &mut TestAppContext) -> Opened<'_> {
    cx.update(|cx| {
        let keymap = r#"
            [[bindings]]
            keys = "ctrl-shift-n"
            command = "notes.show"
            context = "chrome_app"
        "#;
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), keymap, None);
        assert_eq!(diagnostics, []);
        keys::init(cx, Keymap::from_layers([layer.unwrap()]));
    });
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        Workspace::item_panes(APP, window, cx).capabilities(Capabilities::for_os(Os::Linux), cx)
    });
    let mut opened = Opened { workspace, cx };
    opened.draw();
    opened
}

fn item(id: &str, text: &str) -> StatusItem {
    StatusItem::new(id.to_owned(), StatusSide::Left, 0, text.to_owned())
}

#[gpui::test]
fn panels_added_left_right_and_bottom_show_their_docks_with_the_show_key(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let positions = [
        (DockPosition::Left, "notes", Some("notes.show")),
        (DockPosition::Right, "log", None),
        (DockPosition::Bottom, "output", None),
    ];
    for (position, id, show) in positions {
        assert!(
            s.workspace
                .read_with(s.cx, |w, _| w.dock(position).is_none())
        );
        let panel = s.panel(id, show);
        s.add_panel(position, &panel);
    }
    // Each dock shows as its panel is added, with no show or notify.
    for (position, id, _) in positions {
        let dock = s.dock(position);
        let ids = dock.read_with(s.cx, |dock, _| dock.dock().panel_ids());
        assert_eq!(ids, [PanelId(id)]);
        assert!(s.visible(position));
        assert!(s.drawn(position), "{position:?}");
    }

    // The rail's tooltip finds the show key in the workspace's context.
    let dock = s.dock(DockPosition::Left);
    let button = dock.read_with(s.cx, |dock, _| {
        dock.dock()
            .drawn()
            .rail_button_bounds(PanelId("notes"))
            .unwrap()
    });
    s.cx.simulate_mouse_move(button.center(), None, Modifiers::none());
    s.cx.executor().advance_clock(Duration::from_millis(600));
    s.draw();
    let tooltip = dock.read_with(s.cx, |dock, _| dock.dock().drawn().last_tooltip());
    assert!(
        tooltip
            .as_deref()
            .is_some_and(|text| text.starts_with("The notes ") && text.ends_with('N')),
        "{tooltip:?}"
    );
}

#[gpui::test]
fn a_dock_size_can_be_overridden(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let panel = s.panel("notes", None);
    s.add_panel(DockPosition::Left, &panel);
    let workspace = s.workspace.clone();
    s.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.set_dock_size(DockPosition::Left, px(321.), cx)
        })
    });
    let dock = s.dock(DockPosition::Left);
    let size = dock.read_with(s.cx, |dock, _| dock.dock().current_size());
    assert_eq!(size, px(321.));
}

#[gpui::test]
fn hiding_a_dock_with_focus_moves_focus_to_the_active_item(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let page = s.add_page("a", Vec::new());
    let page_focus = page.read_with(s.cx, |page, _| page.focus.clone());
    let panel = s.panel("notes", None);
    s.add_panel(DockPosition::Left, &panel);
    let panel_focus = panel.read_with(s.cx, |panel, _| panel.focus.clone());
    let dock = s.dock(DockPosition::Left);
    s.cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(PanelId("notes"), window, cx));
    });

    // Toggled away.
    s.toggle(DockPosition::Left);
    assert!(!s.visible(DockPosition::Left));
    assert!(!s.drawn(DockPosition::Left));
    s.toggle(DockPosition::Left);
    assert!(s.visible(DockPosition::Left));
    assert!(s.drawn(DockPosition::Left));
    s.focus(&panel_focus);
    assert!(s.focused(&panel_focus));
    s.toggle(DockPosition::Left);
    assert!(!s.visible(DockPosition::Left));
    assert!(s.focused(&page_focus));

    // Hidden by a rail click.
    s.toggle(DockPosition::Left);
    s.focus(&panel_focus);
    assert!(s.focused(&panel_focus));
    let button = dock.read_with(s.cx, |dock, _| {
        dock.dock()
            .drawn()
            .rail_button_bounds(PanelId("notes"))
            .unwrap()
    });
    s.cx.simulate_click(button.center(), Modifiers::none());
    s.draw();
    assert!(!s.visible(DockPosition::Left));
    assert!(s.focused(&page_focus));
}

#[gpui::test]
fn the_status_bar_follows_the_active_item_and_the_workspaces_items(cx: &mut TestAppContext) {
    let mut s = open(cx);
    assert_eq!(s.status_width("own"), None);

    // The active item's, as it becomes active.
    let a = s.add_page("a", vec![item("own", "from a")]);
    assert!(s.status_width("own").is_some());
    let b = s.add_page("b", vec![item("other", "from b")]);
    assert_eq!(s.status_width("own"), None);
    assert!(s.status_width("other").is_some());
    let workspace = s.workspace.clone();
    s.cx.update(|window, cx| {
        let b: Box<dyn ItemHandle> = Box::new(b.clone());
        let a: Box<dyn ItemHandle> = Box::new(a.clone());
        let _ = b;
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.active_pane().clone();
            pane.update(cx, |pane, cx| {
                let ix = pane.pane().items().iter().position(|i| i.id() == a.id());
                pane.activate_item(ix.unwrap(), window, cx);
            });
        });
    });
    s.draw();
    assert!(s.status_width("own").is_some());
    assert_eq!(s.status_width("other"), None);

    // The workspace's own, over the item's of the same id.
    let before = s.status_width("own").unwrap();
    s.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.set_status_item(item("own", "from the workspace, much longer"), cx);
            workspace.set_status_item(item("host", "host"), cx);
        })
    });
    s.draw();
    assert!(s.status_width("own").unwrap() > before);
    assert!(s.status_width("host").is_some());
    s.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.remove_status_item("own", cx);
            workspace.remove_status_item("host", cx);
        })
    });
    s.draw();
    assert_eq!(s.status_width("own"), Some(before));
    assert_eq!(s.status_width("host"), None);
}

#[gpui::test]
fn a_status_source_is_called_as_the_bar_draws(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let workspace = s.workspace.clone();
    s.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.add_status_source(|_| Some(item("pulled", "pulled")), cx)
        })
    });
    s.draw();
    assert!(s.status_width("pulled").is_some());
}
