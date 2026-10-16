// SPDX-License-Identifier: LicenseRef-Proprietary

//! A dock's rail actions: buttons that run a command, pinned to the bottom
//! of a side rail below the panels' buttons, highlighted by the host, with
//! a tooltip like a panel button's.

use std::{cell::Cell, path::Path, rc::Rc, time::Duration};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};
use kiln_core::{Keymap, KeymapLayer};
use kiln_platform::{commands, keys};
use kiln_shell::{Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, RailAction};
use kiln_ui::Icon;

mod term {
    kiln_platform::commands!(
        term,
        [
            /// Toggle
            Toggle,
        ]
    );
}

const ACTION: PanelId = PanelId("terminal");

struct Panel {
    id: PanelId,
    icon: Icon,
    on_rail: bool,
    focus: FocusHandle,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        self.id
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        self.id.0.into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        self.icon
    }

    fn always_on_rail(&self) -> bool {
        self.on_rail
    }
}

struct Host {
    dock: Entity<Dock>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("host")
            .size_full()
            .flex()
            .child(div().flex_1().h_full())
            .child(self.dock.clone())
    }
}

fn open(position: DockPosition, cx: &mut TestAppContext) -> (Entity<Dock>, &mut VisualTestContext) {
    cx.update(|cx| {
        let keymap = r#"
            [[bindings]]
            keys = "ctrl-shift-t"
            command = "term.toggle"
            context = "host"
        "#;
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), keymap, None);
        assert_eq!(diagnostics, []);
        keys::init(cx, Keymap::from_layers([layer.unwrap()]));
        commands::init(cx, "Test");
        term::register(cx);
    });
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let panel = |id, icon, on_rail, cx: &mut Context<Host>| {
            cx.new(|cx| Panel {
                id: PanelId(id),
                icon,
                on_rail,
                focus: cx.focus_handle(),
            })
        };
        let notes = panel("notes", Icon::Files, true, cx);
        let log = panel("log", Icon::ListTree, false, cx);
        let dock = cx.new(|cx| {
            let state = DockState::new(position)
                .size(px(200.))
                .shortcut_contexts(["host"]);
            let mut dock = Dock::with_state(state);
            dock.add_panel(notes, cx);
            dock.add_panel(log, cx);
            dock.add_action(
                RailAction {
                    id: ACTION.0,
                    icon: Icon::FileTerminal,
                    title: "Terminal".into(),
                    command: "term.toggle",
                },
                cx,
            );
            dock
        });
        Host { dock }
    });
    let dock = host.read_with(cx, |host, _| host.dock.clone());
    draw(cx);
    (dock, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Counts the runs of `term.toggle` from now on, as the host's handler would.
fn count_runs(cx: &mut VisualTestContext) -> Rc<Cell<usize>> {
    let runs = Rc::new(Cell::new(0));
    let sink = runs.clone();
    cx.update(|_, cx| {
        cx.on_action(move |_: &term::Toggle, _| sink.set(sink.get() + 1));
    });
    runs
}

#[gpui::test]
fn an_action_is_pinned_below_the_panels_buttons(cx: &mut TestAppContext) {
    let (dock, cx) = open(DockPosition::Right, cx);
    let bounds = |dock: &Entity<Dock>, cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| {
            let drawn = dock.dock().drawn();
            (
                drawn.rail_bounds().unwrap(),
                drawn.rail_button_bounds(PanelId("notes")),
                drawn.rail_button_bounds(ACTION).unwrap(),
                drawn.rail_icons(),
            )
        })
    };
    let (rail, notes, action, icons) = bounds(&dock, cx);
    assert_eq!(icons, [Icon::Files, Icon::FileTerminal]);
    assert!((action.bottom() - rail.bottom()).abs() <= px(1.));
    assert!(notes.unwrap().bottom() <= action.top());

    // A panel that joins later draws above the action, which stays put.
    cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(PanelId("log"), window, cx));
    });
    draw(cx);
    let (rail, _, moved, icons) = bounds(&dock, cx);
    assert_eq!(icons, [Icon::Files, Icon::ListTree, Icon::FileTerminal]);
    assert_eq!(moved, action);
    assert!((moved.bottom() - rail.bottom()).abs() <= px(1.));
    let log = dock.read_with(cx, |dock, _| {
        dock.dock()
            .drawn()
            .rail_button_bounds(PanelId("log"))
            .unwrap()
    });
    assert!(log.bottom() <= moved.top());
}

#[gpui::test]
fn a_click_runs_the_command_and_leaves_the_dock(cx: &mut TestAppContext) {
    let (dock, cx) = open(DockPosition::Right, cx);
    let runs = count_runs(cx);
    let state = |dock: &Entity<Dock>, cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active()))
    };
    let before = state(&dock, cx);
    let action = dock.read_with(cx, |dock, _| dock.dock().drawn().rail_button_bounds(ACTION));
    cx.simulate_click(action.unwrap().center(), Modifiers::none());
    draw(cx);
    assert_eq!(runs.get(), 1);
    assert_eq!(state(&dock, cx), before);

    // Highlighted, or not, as the host says.
    let lit = |dock: &Entity<Dock>, cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| dock.dock().drawn().rail_highlighted())
    };
    assert_eq!(lit(&dock, cx), [PanelId("notes")]);
    dock.update(cx, |dock, cx| {
        dock.set_action_highlighted(ACTION.0, true, cx)
    });
    draw(cx);
    assert_eq!(lit(&dock, cx), [PanelId("notes"), ACTION]);
    dock.update(cx, |dock, cx| {
        dock.set_action_highlighted(ACTION.0, false, cx)
    });
    draw(cx);
    assert_eq!(lit(&dock, cx), [PanelId("notes")]);
}

#[gpui::test]
fn its_tooltip_reads_the_title_and_the_key(cx: &mut TestAppContext) {
    let (dock, cx) = open(DockPosition::Right, cx);
    let button = dock.read_with(cx, |dock, _| dock.dock().drawn().rail_button_bounds(ACTION));
    cx.simulate_mouse_move(button.unwrap().center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    let tooltip = dock.read_with(cx, |dock, _| dock.dock().drawn().last_tooltip());
    assert!(
        tooltip
            .as_deref()
            .is_some_and(|text| text.starts_with("Terminal ") && text.ends_with('T')),
        "{tooltip:?}"
    );
}

#[gpui::test]
fn a_bottom_dock_draws_its_actions_after_its_buttons(cx: &mut TestAppContext) {
    let (dock, cx) = open(DockPosition::Bottom, cx);
    let icons = dock.read_with(cx, |dock, _| dock.dock().drawn().rail_icons());
    assert_eq!(icons, [Icon::Files, Icon::FileTerminal]);
    let (notes, action) = dock.read_with(cx, |dock, _| {
        let drawn = dock.dock().drawn();
        (
            drawn.rail_button_bounds(PanelId("notes")).unwrap(),
            drawn.rail_button_bounds(ACTION).unwrap(),
        )
    });
    assert!(notes.right() <= action.left());
}
