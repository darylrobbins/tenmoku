// SPDX-License-Identifier: Apache-2.0

//! Panels moved between docks: taken out of one and put in another, the
//! same entity, focus handle and state; and dragged between the docks of
//! one transfer group, a tabbed dock's tab onto a side dock's column or its
//! rail's top group, a side rail's button onto a tabbed dock's groups or
//! its own rail's foot, each drop told to the dock's host. Docks without a
//! shared group ignore each other's drags.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AppContext as _, Bounds, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement,
    Modifiers, MouseButton, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _,
    TestAppContext, VisualTestContext, Window, canvas, div, point, px, size,
};
use kiln_command::Value;
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, RailAction,
    dock::{DockDrop, DockEvent, DrawnDock, track_centre},
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};
use kiln_ui::Icon;

const A: PanelId = PanelId("a");
const B: PanelId = PanelId("b");
const X: PanelId = PanelId("x");
const Y: PanelId = PanelId("y");
const Z: PanelId = PanelId("z");

const TOOLS: &str = "tools";

/// A panel with state of its own, which records where it was laid out.
struct Panel {
    id: PanelId,
    focus: FocusHandle,
    state: Rc<Cell<u32>>,
    laid_out: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let laid_out = self.laid_out.clone();
        div().size_full().child(
            canvas(move |bounds, _, _| laid_out.set(Some(bounds)), |_, _, _, _| {}).size_full(),
        )
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
        format!("The {}", self.id.0).into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        Icon::Files
    }

    fn save_state(&self, _: &gpui::App) -> Option<Value> {
        Some(Value::from(self.state.get()))
    }

    fn restore_state(&mut self, saved: &Value, _: &mut Context<Self>) {
        self.state.set(saved.as_u64().unwrap_or_default() as u32);
    }
}

fn panel(id: PanelId, made: &mut Vec<Entity<Panel>>, cx: &mut gpui::App) -> Entity<Panel> {
    let panel = cx.new(|cx| Panel {
        id,
        focus: cx.focus_handle(),
        state: Rc::default(),
        laid_out: Rc::default(),
    });
    made.push(panel.clone());
    panel
}

/// A side dock on the left of `left`, in the transfer group, with a rail
/// action at its foot; a tabbed bottom dock of `bottom`, each shown, in
/// the group too; and a side dock on the right of `right`, in no group.
struct Host {
    left: Entity<Dock>,
    bottom: Entity<Dock>,
    right: Entity<Dock>,
    panels: Vec<Entity<Panel>>,
    events: Rc<RefCell<Vec<(&'static str, DockEvent)>>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let centre = div()
            .relative()
            .flex_1()
            .min_w(MIN_PANE_WIDTH)
            .h_full()
            .child(track_centre());
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h(MIN_PANE_HEIGHT)
                    .w_full()
                    .flex()
                    .child(self.left.clone())
                    .child(centre)
                    .child(self.right.clone()),
            )
            .child(self.bottom.clone())
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        for _ in 0..4 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
            if self.cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
                return;
            }
        }
    }

    fn dock(&mut self, which: &str) -> Entity<Dock> {
        self.host.read_with(self.cx, |host, _| match which {
            "left" => host.left.clone(),
            "bottom" => host.bottom.clone(),
            _ => host.right.clone(),
        })
    }

    fn panel(&mut self, id: PanelId) -> Entity<Panel> {
        let panels = self.host.read_with(self.cx, |host, _| host.panels.clone());
        (panels.into_iter())
            .find(|panel| panel.read_with(self.cx, |panel, _| panel.id) == id)
            .expect("made")
    }

    fn id(&mut self, which: &str) -> EntityId {
        self.dock(which).entity_id()
    }

    fn drawn(&mut self, which: &str) -> DrawnDock {
        let dock = self.dock(which);
        dock.read_with(self.cx, |dock, _| dock.dock().drawn().clone())
    }

    fn events(&mut self) -> Vec<(&'static str, DockEvent)> {
        self.host
            .read_with(self.cx, |host, _| host.events.borrow().clone())
    }

    /// Presses at `from`, drags it to `to`, holding it there.
    fn hover(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(from, MouseButton::Left, none);
        let nudged = point(from.x + px(3.), from.y + px(3.));
        self.cx.simulate_mouse_move(nudged, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
    }

    fn release(&mut self, at: Point<Pixels>) {
        self.cx
            .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        self.draw();
    }

    fn tab(&mut self, panel: PanelId) -> Point<Pixels> {
        let drawn = self.drawn("bottom");
        drawn.tab_bounds(panel).expect("a tab").center()
    }

    fn rail_button(&mut self, which: &str, panel: PanelId) -> Bounds<Pixels> {
        let drawn = self.drawn(which);
        drawn.rail_button_bounds(panel).expect("a rail button")
    }
}

/// The three docks, drawn.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let (host, cx) = cx.add_window_view(|window, cx| {
        let mut made = Vec::new();
        let made = &mut made;
        let left = cx.new(|cx| {
            let state = DockState::new(DockPosition::Left)
                .transfer_group(TOOLS)
                .size(px(240.));
            let mut dock = Dock::with_state(state);
            dock.add_panel(panel(X, made, cx), cx);
            dock.add_panel(panel(Y, made, cx), cx);
            dock.add_action(
                RailAction {
                    id: "run",
                    icon: Icon::Play,
                    title: "Run".into(),
                    command: "run.show",
                },
                cx,
            );
            dock
        });
        let bottom = cx.new(|cx| {
            let state = DockState::new(DockPosition::Bottom)
                .without_rail()
                .with_tabs()
                .transfer_group(TOOLS)
                .size(px(300.));
            let mut dock = Dock::with_state(state);
            for id in [A, B] {
                dock.add_panel(panel(id, made, cx), cx);
                dock.show(id, window, cx);
            }
            dock
        });
        let right = cx.new(|cx| {
            let mut dock = Dock::with_state(DockState::new(DockPosition::Right).size(px(240.)));
            dock.add_panel(panel(Z, made, cx), cx);
            dock
        });
        let events = Rc::<RefCell<Vec<_>>>::default();
        for (name, dock) in [("left", &left), ("bottom", &bottom), ("right", &right)] {
            let sink = events.clone();
            cx.subscribe(dock, move |_, _, event: &DockEvent, _| {
                sink.borrow_mut().push((name, *event))
            })
            .detach();
        }
        Host {
            left,
            bottom,
            right,
            panels: made.clone(),
            events,
        }
    });
    cx.simulate_resize(size(px(1400.), px(900.)));
    let mut opened = Opened { host, cx };
    opened.draw();
    opened
}

fn ids(dock: &DockState) -> Vec<PanelId> {
    dock.panel_ids()
}

#[gpui::test]
fn a_panel_moved_between_docks_keeps_its_entity_and_state(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (bottom, left) = (s.dock("bottom"), s.dock("left"));
    let b = s.panel(B);
    let focus = b.read_with(s.cx, |panel, _| panel.focus.clone());
    b.read_with(s.cx, |panel, _| panel.state.set(7));
    let saved = s.cx.update(|_, cx| bottom.read(cx).dock().save(cx));
    assert_eq!(saved["panels"]["b"], Value::from(7));
    assert_eq!(s.drawn("bottom").tabs(), [A, B]);

    // B moves from the bottom dock to the left one, between X and Y.
    let moved = s.cx.update(|window, cx| {
        let taken = bottom.update(cx, |dock, cx| {
            let taken = dock.dock_mut().take_panel(B).expect("registered");
            cx.notify();
            taken
        });
        assert_eq!(taken.id(), B);
        left.update(cx, |dock, cx| {
            let id = dock.dock_mut().insert_panel(taken, 1);
            dock.dock_mut().show(id, window, cx);
            cx.notify();
            id
        })
    });
    assert_eq!(moved, B);
    b.read_with(s.cx, |panel, _| panel.laid_out.set(None));
    s.draw();

    // The source no longer lists it, nor draws its tab; A shows in its
    // place.
    assert_eq!(bottom.read_with(s.cx, |dock, _| ids(dock.dock())), [A]);
    assert_eq!(s.drawn("bottom").tabs(), [A]);
    assert_eq!(bottom.read_with(s.cx, |dock, _| dock.active()), Some(A));
    assert!(bottom.read_with(s.cx, |dock, _| dock.is_visible()));
    let gone = s.cx.update(|_, cx| bottom.read(cx).dock().panel_focus(B, cx));
    assert!(gone.is_none());
    let saved = s.cx.update(|_, cx| bottom.read(cx).dock().save(cx));
    assert!(saved["panels"].get("b").is_none(), "{saved}");
    // The target does, on its rail in its place, shown: the same entity,
    // drawn, with the same focus handle and state.
    assert_eq!(left.read_with(s.cx, |dock, _| ids(dock.dock())), [X, B, Y]);
    assert_eq!(s.drawn("left").rail_ids(), [X, B, Y, PanelId("run")]);
    assert_eq!(left.read_with(s.cx, |dock, _| dock.active()), Some(B));
    assert!(b.read_with(s.cx, |panel, _| panel.laid_out.get().is_some()));
    let title = s.cx.update(|_, cx| left.read(cx).dock().panel_title(B, cx));
    assert_eq!(title.as_deref(), Some("The b"));
    let held = s.cx.update(|_, cx| left.read(cx).dock().panel_focus(B, cx));
    assert_eq!(held, Some(focus.clone()));
    let saved = s.cx.update(|_, cx| left.read(cx).dock().save(cx));
    assert_eq!(saved["panels"]["b"], Value::from(7));

    // Moved back, at the end, it's still that entity, its state kept.
    s.cx.update(|window, cx| {
        let taken = left.update(cx, |dock, cx| {
            let taken = dock.dock_mut().take_panel(B).unwrap();
            cx.notify();
            taken
        });
        bottom.update(cx, |dock, cx| {
            dock.dock_mut().insert_panel(taken, usize::MAX);
            dock.dock_mut().show(B, window, cx);
            cx.notify();
        });
    });
    s.draw();
    assert_eq!(left.read_with(s.cx, |dock, _| ids(dock.dock())), [X, Y]);
    assert_eq!(left.read_with(s.cx, |dock, _| dock.active()), Some(X));
    assert_eq!(s.drawn("left").rail_ids(), [X, Y, PanelId("run")]);
    assert_eq!(s.drawn("bottom").tabs(), [A, B]);
    let held = s.cx.update(|_, cx| bottom.read(cx).dock().panel_focus(B, cx));
    assert_eq!(held, Some(focus));
    let saved = s.cx.update(|_, cx| bottom.read(cx).dock().save(cx));
    assert_eq!(saved["panels"]["b"], Value::from(7));
    // Nothing's left to take.
    assert!(left.update(s.cx, |dock, _| dock.dock_mut().take_panel(B)).is_none());

    // remove_action takes the rail action off.
    let removed = left.update(s.cx, |dock, cx| {
        cx.notify();
        dock.dock_mut().remove_action("run")
    });
    assert!(removed);
    s.draw();
    assert_eq!(s.drawn("left").rail_ids(), [X, Y]);
    assert!(!left.update(s.cx, |dock, _| dock.dock_mut().remove_action("run")));
}

#[gpui::test]
fn drags_cross_only_between_docks_of_one_transfer_group(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (bottom_id, left_id) = (s.id("bottom"), s.id("left"));

    // The bottom dock's tab over the side dock's column: into it, the
    // column tinted; dropped, the side dock tells its host.
    let column = s.drawn("left").column_bounds().expect("shown");
    let from = s.tab(B);
    s.hover(from, column.center());
    let drawn = s.drawn("left");
    assert_eq!(drawn.drop_hint(), Some(DockDrop::Into));
    assert_eq!(drawn.drop_hint_bounds(), Some(column));
    assert_eq!(s.drawn("bottom").drop_hint(), None);
    s.release(column.center());
    assert_eq!(s.drawn("left").drop_hint(), None);
    assert_eq!(
        s.events(),
        [(
            "left",
            DockEvent::PanelDropped {
                panel: B,
                from: bottom_id,
                target: DockDrop::Into,
            }
        )]
    );
    // The host moves it: nothing moved yet.
    assert_eq!(s.drawn("bottom").tabs(), [A, B]);

    // Over the side rail's top group, too.
    let x = s.rail_button("left", X);
    let from = s.tab(A);
    s.hover(from, x.center());
    let drawn = s.drawn("left");
    assert_eq!(drawn.drop_hint(), Some(DockDrop::Into));
    let hint = drawn.drop_hint_bounds().expect("drawn");
    assert!(hint.contains(&x.center()) && hint.left() == x.left(), "{hint:?} {x:?}");
    let run = s.rail_button("left", PanelId("run"));
    assert!(!hint.contains(&run.center()), "{hint:?}");
    s.release(x.center());
    assert_eq!(
        s.events()[1],
        (
            "left",
            DockEvent::PanelDropped {
                panel: A,
                from: bottom_id,
                target: DockDrop::Into,
            }
        )
    );

    // The side rail's button over the tabbed dock's group: its zones.
    let content = s.drawn("bottom").group_content_bounds(0).expect("drawn");
    let y = s.rail_button("left", Y).center();
    s.hover(y, content.center());
    assert_eq!(s.drawn("bottom").drop_hint(), Some(DockDrop::Join(0)));
    assert_eq!(s.drawn("left").drop_hint(), None);
    let right_quarter = point(content.right() - px(20.), content.center().y);
    s.cx.simulate_mouse_move(right_quarter, MouseButton::Left, Modifiers::none());
    s.draw();
    assert_eq!(s.drawn("bottom").drop_hint(), Some(DockDrop::SplitAfter(0)));
    let strip = s.drawn("bottom").group_strip_bounds(0).expect("drawn");
    let after_b = s.drawn("bottom").tab_bounds(B).expect("drawn");
    let end = point(after_b.right() + px(10.), strip.center().y);
    s.cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
    s.draw();
    assert_eq!(
        s.drawn("bottom").drop_hint(),
        Some(DockDrop::Insert { group: 0, index: 2 })
    );
    assert!(s.drawn("bottom").drop_hint_bounds().is_some());
    s.release(end);
    assert_eq!(
        s.events()[2],
        (
            "bottom",
            DockEvent::PanelDropped {
                panel: Y,
                from: left_id,
                target: DockDrop::Insert { group: 0, index: 2 },
            }
        )
    );
    // A click on it still shows its panel.
    let y = s.rail_button("left", Y).center();
    s.cx.simulate_click(y, Modifiers::none());
    s.draw();
    assert_eq!(s.dock("left").read_with(s.cx, |dock, _| dock.active()), Some(Y));

    // The side rail's button over its own foot group: into the bottom
    // dock, for the host.
    let run = s.rail_button("left", PanelId("run"));
    let x = s.rail_button("left", X).center();
    s.hover(x, run.center());
    let drawn = s.drawn("left");
    assert_eq!(drawn.drop_hint(), Some(DockDrop::Foot));
    let hint = drawn.drop_hint_bounds().expect("drawn");
    assert!(hint.contains(&run.center()), "{hint:?}");
    assert!(!hint.contains(&x), "{hint:?}");
    s.release(run.center());
    assert_eq!(
        s.events()[3],
        (
            "left",
            DockEvent::PanelDropped {
                panel: X,
                from: left_id,
                target: DockDrop::Foot,
            }
        )
    );
    // Its own button over its own column takes nothing.
    let column = s.drawn("left").column_bounds().expect("shown");
    let x = s.rail_button("left", X).center();
    s.hover(x, column.center());
    assert_eq!(s.drawn("left").drop_hint(), None);
    s.release(column.center());
    assert_eq!(s.events().len(), 4);

    // The dock without a group ignores the others' drags, and its rail's
    // buttons don't drag.
    let right = s.drawn("right").column_bounds().expect("shown");
    let from = s.tab(A);
    s.hover(from, right.center());
    assert_eq!(s.drawn("right").drop_hint(), None);
    assert_eq!(s.drawn("bottom").drop_hint(), None);
    s.release(right.center());
    let x = s.rail_button("left", X).center();
    s.hover(x, right.center());
    assert_eq!(s.drawn("right").drop_hint(), None);
    s.release(right.center());
    let z = s.rail_button("right", Z).center();
    s.hover(z, content.center());
    assert_eq!(s.drawn("bottom").drop_hint(), None);
    s.release(content.center());
    assert_eq!(s.events().len(), 4);
}
