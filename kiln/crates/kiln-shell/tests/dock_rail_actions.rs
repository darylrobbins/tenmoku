// SPDX-License-Identifier: Apache-2.0

//! A dock's rail actions: buttons that run a command, pinned to the bottom
//! of a side rail below the panels' buttons, highlighted by the host, with
//! a tooltip like a panel button's; the foot panels above them; and the
//! badges the host sets on its panels' buttons.

use std::{cell::Cell, path::Path, rc::Rc, time::Duration};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};
use kiln_core::{Keymap, KeymapLayer};
use kiln_platform::{commands, keys};
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, RailAction, dock::RailBadge,
};
use kiln_ui::{BadgeTone, Icon, RAIL_WIDTH, RailButton};

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
    foot: bool,
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

    fn on_rail_foot(&self) -> bool {
        self.foot
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
                foot: false,
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
    // The rail's padding below its last button.
    let foot = px(4.);
    assert!((action.bottom() - rail.bottom()).abs() <= foot);
    assert!(notes.unwrap().bottom() <= action.top());

    // A panel that joins later draws above the action, which stays put.
    cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(PanelId("log"), window, cx));
    });
    draw(cx);
    let (rail, _, moved, icons) = bounds(&dock, cx);
    assert_eq!(icons, [Icon::Files, Icon::ListTree, Icon::FileTerminal]);
    assert_eq!(moved, action);
    assert!((moved.bottom() - rail.bottom()).abs() <= foot);
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

/// Adds a panel whose button is in the rail's foot, always on the rail.
fn add_foot_panel(dock: &Entity<Dock>, cx: &mut VisualTestContext) {
    dock.update(cx, |dock, cx| {
        let problems = cx.new(|cx| Panel {
            id: PanelId("problems"),
            icon: Icon::TriangleAlert,
            on_rail: true,
            foot: true,
            focus: cx.focus_handle(),
        });
        dock.add_panel(problems, cx);
    });
    draw(cx);
}

#[gpui::test]
fn a_foot_panel_is_above_the_actions(cx: &mut TestAppContext) {
    let (dock, cx) = open(DockPosition::Left, cx);
    add_foot_panel(&dock, cx);
    let (icons, notes, problems, action, rail) = dock.read_with(cx, |dock, _| {
        let drawn = dock.dock().drawn();
        (
            drawn.rail_icons(),
            drawn.rail_button_bounds(PanelId("notes")).unwrap(),
            drawn.rail_button_bounds(PanelId("problems")).unwrap(),
            drawn.rail_button_bounds(ACTION).unwrap(),
            drawn.rail_bounds().unwrap(),
        )
    });
    assert_eq!(icons, [Icon::Files, Icon::TriangleAlert, Icon::FileTerminal]);
    assert_eq!(problems.bottom(), action.top());
    assert!(notes.bottom() < problems.top());
    // 42 px wide, 32 px items, left of the panel.
    let column = dock.read_with(cx, |dock, _| dock.dock().drawn().column_bounds().unwrap());
    assert_eq!(rail.right(), column.left());
    assert_eq!(rail.size.width, RAIL_WIDTH);
    assert_eq!(problems.size, gpui::size(RAIL_WIDTH, RailButton::HEIGHT));

    // Shown, it's highlighted like any panel, its bar on the rail's right
    // edge, facing the panel.
    cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(PanelId("problems"), window, cx));
    });
    draw(cx);
    let lit = dock.read_with(cx, |dock, _| dock.dock().drawn().rail_highlighted());
    assert_eq!(lit, [PanelId("problems")]);
    let bar = cx.debug_bounds("rail-bar").expect("a bar");
    assert_eq!((bar.right(), bar.size.width), (rail.right(), px(2.)));
}

#[gpui::test]
fn the_host_badges_a_panel_button(cx: &mut TestAppContext) {
    let (dock, cx) = open(DockPosition::Right, cx);
    add_foot_panel(&dock, cx);
    let badges = |dock: &Entity<Dock>, cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| dock.dock().drawn().rail_badges())
    };
    assert_eq!(badges(&dock, cx), []);
    let count = RailBadge::count(3, BadgeTone::Modified, "3 changes");
    let dot = RailBadge::dot(BadgeTone::Error, "1 error");
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_badge(PanelId("notes"), Some(count.clone()));
        cx.notify();
    });
    draw(cx);
    // Drawn in its button.
    let notes = dock.read_with(cx, |dock, _| {
        dock.dock().drawn().rail_button_bounds(PanelId("notes")).unwrap()
    });
    let badge = cx.debug_bounds("badge").expect("drawn");
    assert!(notes.contains(&badge.origin), "{badge:?} in {notes:?}");
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_badge(PanelId("problems"), Some(dot.clone()));
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        badges(&dock, cx),
        [(PanelId("notes"), count), (PanelId("problems"), dot.clone())]
    );

    // Taken off.
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_badge(PanelId("notes"), None);
        cx.notify();
    });
    draw(cx);
    assert_eq!(badges(&dock, cx), [(PanelId("problems"), dot)]);

    // On the right edge, the bar faces the panel on the rail's left.
    let rail = dock.read_with(cx, |dock, _| dock.dock().drawn().rail_bounds().unwrap());
    let bar = cx.debug_bounds("rail-bar").expect("a bar");
    assert_eq!(bar.left(), rail.left());
}

/// Whether `a` and `b` share any area.
fn overlap(a: gpui::Bounds<gpui::Pixels>, b: gpui::Bounds<gpui::Pixels>) -> bool {
    a.left() < b.right() && b.left() < a.right() && a.top() < b.bottom() && b.top() < a.bottom()
}

#[gpui::test]
fn badges_stay_clear_of_the_glyphs(cx: &mut TestAppContext) {
    for position in [DockPosition::Left, DockPosition::Right] {
        let mut app = cx.clone();
        let (dock, cx) = open(position, &mut app);
        // `log` joins under `notes`, so a badge on `notes` has an icon
        // below it.
        cx.update(|window, cx| {
            dock.update(cx, |dock, cx| dock.show(PanelId("log"), window, cx));
            dock.update(cx, |dock, cx| dock.show(PanelId("notes"), window, cx));
        });
        draw(cx);
        let cases = [
            RailBadge::count(3, BadgeTone::Modified, "3"),
            RailBadge::count(42, BadgeTone::Modified, "42"),
            RailBadge::count(120, BadgeTone::Modified, "99+"),
            RailBadge::dot(BadgeTone::Error, "errors"),
        ];
        for badge in cases {
            let name = badge.name.clone();
            dock.update(cx, |dock, cx| {
                dock.dock_mut().set_badge(PanelId("notes"), Some(badge));
                cx.notify();
            });
            draw(cx);
            let (rail, notes) = dock.read_with(cx, |dock, _| {
                let drawn = dock.dock().drawn();
                let notes = drawn.rail_button_bounds(PanelId("notes")).unwrap();
                (drawn.rail_bounds().unwrap(), notes)
            });
            let outer = cx.debug_bounds("badge").expect("drawn");
            let ring = kiln_ui::BADGE_RING;
            let fill = gpui::Bounds::from_corners(
                outer.origin + gpui::point(ring, ring),
                outer.bottom_right() - gpui::point(ring, ring),
            );
            let icon = cx.debug_bounds("rail-icon:dock-rail-0").expect("notes' icon");
            let below = cx.debug_bounds("rail-icon:dock-rail-1").expect("log's icon");
            let at = format!("{position:?} {name}: {fill:?}, icon {icon:?}");
            assert_eq!(icon.size, gpui::size(px(16.), px(16.)), "{at}");
            assert!(!overlap(fill, icon), "{at}");
            assert!(!overlap(outer, below), "{at}, next icon {below:?}");
            // Bottom-right of the icon, inside the rail and clear of its
            // selection bar on either side.
            assert!(fill.right() > icon.center().x && fill.top() >= icon.bottom(), "{at}");
            assert!(outer.left() >= rail.left() + px(2.), "{at}");
            assert!(outer.right() <= rail.right() - px(2.), "{at}");
            assert!(fill.top() < notes.bottom(), "{at}");
        }
    }
}
