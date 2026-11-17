// SPDX-License-Identifier: Apache-2.0

//! A tabbed dock's groups: a tab dragged to the outer quarter of a group's
//! content splits a group off beside it, dropped on another group's strip
//! joins it at the pointer, dragged along its own strip reorders it, and
//! dragged out as a group's last takes the group away; a split that would
//! leave a half too narrow is refused; the border between groups drags
//! their widths and a double-click evens them; hiding a shown panel
//! collapses its group alone, closing a group's last tab hides it in its
//! place; the groups are saved and restored, stack at a side, and are
//! named for assistive tech.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AppContext as _, Bounds, Context, CursorStyle, Entity, FocusHandle, Focusable, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
    Render, SharedString, Styled as _, TestAppContext, VisualTestContext, Window, canvas, div,
    point, px, size,
};
use kiln_command::Value;
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId,
    dock::{
        DockDrop, DockEvent, DrawnDock, MIN_DOCK_HEIGHT, MIN_DOCK_WIDTH, MoveTo, track_centre,
    },
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};
use kiln_ui::{Icon, Role};

const A: PanelId = PanelId("a");
const B: PanelId = PanelId("b");
const C: PanelId = PanelId("c");
const D: PanelId = PanelId("d");

/// A panel that records where it was last laid out.
struct Panel {
    id: PanelId,
    focus: FocusHandle,
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
}

/// A centre with the dock at its position, as the shell places it, and
/// what the dock told it: its events, and how many times it notified.
struct Host {
    dock: Entity<Dock>,
    position: DockPosition,
    events: Rc<RefCell<Vec<DockEvent>>>,
    notified: Rc<Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let at = |position| (self.position == position).then(|| self.dock.clone());
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
                    .children(at(DockPosition::Left))
                    .child(centre)
                    .children(at(DockPosition::Right)),
            )
            .children(at(DockPosition::Bottom))
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    dock: Entity<Dock>,
    panels: Vec<Entity<Panel>>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        for panel in self.panels.clone() {
            panel.read_with(self.cx, |panel, _| panel.laid_out.set(None));
        }
        for _ in 0..4 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
            if self.cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
                return;
            }
        }
    }

    fn drawn(&mut self) -> DrawnDock {
        self.dock
            .read_with(self.cx, |dock, _| dock.dock().drawn().clone())
    }

    fn read<R>(&mut self, read: impl FnOnce(&DockState) -> R) -> R {
        self.dock.read_with(self.cx, |dock, _| read(dock.dock()))
    }

    /// Changes the dock's state, then draws.
    fn update<R>(
        &mut self,
        update: impl FnOnce(&mut DockState, &mut Window, &mut gpui::App) -> R,
    ) -> R {
        let dock = self.dock.clone();
        let result = self.cx.update(|window, cx| {
            dock.update(cx, |dock, cx| {
                let result = update(dock.dock_mut(), window, cx);
                cx.notify();
                result
            })
        });
        self.draw();
        result
    }

    fn save(&mut self) -> Value {
        self.dock.read_with(self.cx, |dock, cx| dock.dock().save(cx))
    }

    /// The panels whose views the last frame drew.
    fn panels_drawn(&mut self) -> Vec<PanelId> {
        (self.panels.clone().into_iter())
            .filter_map(|panel| {
                panel.read_with(self.cx, |panel, _| panel.laid_out.get().map(|_| panel.id))
            })
            .collect()
    }

    /// Presses on `panel`'s tab and drags it to `to`, holding it there.
    fn hover(&mut self, panel: PanelId, to: Point<Pixels>) {
        let from = self.drawn().tab_bounds(panel).expect("drawn").center();
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(from, MouseButton::Left, none);
        let nudged = point(from.x + px(3.), from.y + px(3.));
        self.cx.simulate_mouse_move(nudged, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
    }

    /// Moves a held drag on to `to`.
    fn hover_on(&mut self, to: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        self.draw();
    }

    fn release(&mut self, at: Point<Pixels>) {
        self.cx
            .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        self.draw();
    }

    fn drag_tab(&mut self, panel: PanelId, to: Point<Pixels>) {
        self.hover(panel, to);
        self.release(to);
    }

    /// Drags from `at` to `to`, as a border's handle is.
    fn drag(&mut self, at: Point<Pixels>, to: Point<Pixels>) {
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(at, MouseButton::Left, none);
        let nudged = at + (to - at) * 0.5;
        self.cx.simulate_mouse_move(nudged, MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_up(to, MouseButton::Left, none);
        self.draw();
    }

    fn double_click(&mut self, at: Point<Pixels>) {
        for click_count in [1, 2] {
            self.cx.simulate_event(MouseDownEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count,
            });
        }
        self.draw();
    }

    fn move_to(&mut self, position: DockPosition) {
        self.dock.update(self.cx, |dock, cx| {
            dock.dock_mut().set_position(position);
            cx.notify();
        });
        self.host.update(self.cx, |host, cx| {
            host.position = position;
            cx.notify();
        });
        self.draw();
    }

    fn width(&mut self, group: usize) -> Pixels {
        self.drawn().group_bounds(group).expect("drawn").size.width
    }

    fn height(&mut self, group: usize) -> Pixels {
        self.drawn().group_bounds(group).expect("drawn").size.height
    }

    fn panel(&mut self, id: PanelId) -> Entity<Panel> {
        (self.panels.clone().into_iter())
            .find(|panel| panel.read_with(self.cx, |panel, _| panel.id) == id)
            .expect("registered")
    }

    fn focus(&mut self, id: PanelId) {
        let panel = self.panel(id);
        self.cx.update(|window, cx| {
            let focus = panel.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
    }

    fn events(&mut self) -> Vec<DockEvent> {
        self.host
            .read_with(self.cx, |host, _| host.events.borrow().clone())
    }

    fn notified(&mut self) -> usize {
        self.host.read_with(self.cx, |host, _| host.notified.get())
    }

    /// The labels of the nodes of `role` the next frame draws.
    fn labels(&mut self, role: Role) -> Vec<String> {
        self.cx.update(|_, cx| kiln_ui::clear_nodes(cx));
        self.dock.update(self.cx, |_, cx| cx.notify());
        self.draw();
        let nodes = self.cx.update(|window, cx| kiln_ui::nodes(window, cx));
        (nodes.iter())
            .filter(|node| node.role == role)
            .map(|node| node.label.to_string())
            .collect()
    }

    fn hidden_panel_focused(&mut self) -> bool {
        let dock = self.dock.clone();
        self.cx
            .update(|window, cx| dock.read(cx).dock().hidden_panel_focused(window, cx))
    }

    /// A dock of `ids` in the same window, not drawn, as a restore into a
    /// new window would find it.
    fn fresh(&mut self, ids: &[PanelId]) -> Entity<Dock> {
        let ids = ids.to_vec();
        self.cx
            .update(|_, cx| cx.new(|cx| dock_of(DockPosition::Bottom, &ids, &mut Vec::new(), cx)))
    }
}

/// A tabbed dock at `position` without a rail, 300 px across, of a panel
/// for each of `ids`, which go in `panels`.
fn dock_of(
    position: DockPosition,
    ids: &[PanelId],
    panels: &mut Vec<Entity<Panel>>,
    cx: &mut Context<Dock>,
) -> Dock {
    let state = DockState::new(position)
        .without_rail()
        .with_tabs()
        .size(px(300.));
    let mut dock = Dock::with_state(state);
    for &id in ids {
        let panel = cx.new(|cx| Panel {
            id,
            focus: cx.focus_handle(),
            laid_out: Rc::default(),
        });
        panels.push(panel.clone());
        dock.add_panel(panel, cx);
    }
    dock
}

/// A window with a tabbed dock at the bottom of `ids`, each shown in turn,
/// so each has a tab and the last is active, drawn.
fn open<'a>(ids: &[PanelId], cx: &'a mut TestAppContext) -> Opened<'a> {
    let ids = ids.to_vec();
    let made = Rc::new(RefCell::new(Vec::new()));
    let sink = made.clone();
    let (host, cx) = cx.add_window_view(move |window, cx| {
        let dock = cx.new(|cx| {
            let mut dock = dock_of(DockPosition::Bottom, &ids, &mut sink.borrow_mut(), cx);
            for &id in &ids {
                dock.show(id, window, cx);
            }
            dock
        });
        let (events, notified) = (Rc::<RefCell<Vec<_>>>::default(), Rc::<Cell<_>>::default());
        let (sink, count) = (events.clone(), notified.clone());
        cx.subscribe(&dock, move |_, _, event: &DockEvent, _| sink.borrow_mut().push(*event))
            .detach();
        cx.observe(&dock, move |_, _, _| count.set(count.get() + 1))
            .detach();
        Host {
            dock,
            position: DockPosition::Bottom,
            events,
            notified,
        }
    });
    cx.simulate_resize(size(px(1200.), px(900.)));
    let dock = host.read_with(cx, |host, _| host.dock.clone());
    let panels = made.take();
    let mut opened = Opened {
        host,
        dock,
        panels,
        cx,
    };
    opened.draw();
    opened
}

/// The pointer in the trailing quarter of `group`'s content along the
/// row, half way across it.
fn right_quarter(drawn: &DrawnDock, group: usize) -> Point<Pixels> {
    let content = drawn.group_content_bounds(group).expect("drawn");
    point(content.right() - px(20.), content.center().y)
}

fn left_quarter(drawn: &DrawnDock, group: usize) -> Point<Pixels> {
    let content = drawn.group_content_bounds(group).expect("drawn");
    point(content.left() + px(20.), content.center().y)
}

fn centre(drawn: &DrawnDock, group: usize) -> Point<Pixels> {
    drawn.group_content_bounds(group).expect("drawn").center()
}

/// The pointer between `left`'s tab and the one after it.
fn between(drawn: &DrawnDock, left: PanelId) -> Point<Pixels> {
    let tab = drawn.tab_bounds(left).expect("drawn");
    point(tab.right() - px(2.), tab.center().y)
}

fn close(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= 1.5
}

#[gpui::test]
fn a_tab_dropped_on_a_groups_right_quarter_splits_it_into_a_group_beside(
    cx: &mut TestAppContext,
) {
    let mut s = open(&[A, B, C], cx);
    let drawn = s.drawn();
    assert_eq!(drawn.tabs(), [A, B, C]);
    assert_eq!(s.read(|dock| dock.group_count()), 1);
    let whole = drawn.group_bounds(0).expect("drawn");

    let to = right_quarter(&drawn, 0);
    s.hover(B, to);
    let hovered = s.drawn();
    assert_eq!(hovered.drop_hint(), Some(DockDrop::SplitAfter(0)));
    let hint = hovered.drop_hint_bounds().expect("drawn");
    assert!(close(hint.left(), whole.center().x), "{hint:?} {whole:?}");
    assert!(close(hint.right(), whole.right()), "{hint:?} {whole:?}");
    assert!(close(hint.top(), whole.top()) && close(hint.bottom(), whole.bottom()));
    s.release(to);

    let drawn = s.drawn();
    assert_eq!(drawn.drop_hint(), None);
    assert_eq!(drawn.drop_hint_bounds(), None);
    assert_eq!(s.read(|dock| dock.group_count()), 2);
    assert_eq!(drawn.drawn_groups(), [0, 1]);
    assert_eq!(drawn.group_tabs(0), [A, C]);
    assert_eq!(drawn.group_tabs(1), [B]);
    assert_eq!(drawn.tabs(), [A, C, B]);
    assert_eq!(s.read(|dock| dock.open_panels()), [A, C, B]);
    // Side by side, each with its own strip, filling the column.
    let left = drawn.group_strip_bounds(0).unwrap();
    let right = drawn.group_strip_bounds(1).unwrap();
    assert!(left.right() <= right.left(), "{left:?} {right:?}");
    assert_eq!(left.top(), right.top());
    let left = drawn.group_bounds(0).unwrap();
    let right = drawn.group_bounds(1).unwrap();
    assert!(left.right() <= right.left());
    assert!(close(left.left(), whole.left()) && close(right.right(), whole.right()));
    // Both groups' active panels drawn at once.
    assert_eq!(s.read(|dock| dock.shown_panels()), [C, B]);
    let mut shown = s.panels_drawn();
    shown.sort_by_key(|id| id.0);
    assert_eq!(shown, [B, C]);
    assert!(s.read(|dock| dock.is_shown(B) && dock.is_shown(C) && !dock.is_shown(A)));
    // The dropped panel is active, its group focused.
    assert_eq!(s.read(|dock| dock.active()), Some(B));
    assert_eq!(s.read(|dock| dock.group_of(B)), Some(1));
    assert_eq!(s.read(|dock| dock.open_panels_in(1)), [B]);
    // The next group, wrapping, focused with its panel.
    let next = s.update(|dock, window, cx| dock.focus_next_group(1, window, cx));
    assert_eq!(next, Some(C));
    assert_eq!(s.read(|dock| dock.active()), Some(C));
    let next = s.update(|dock, window, cx| dock.focus_next_group(-1, window, cx));
    assert_eq!(next, Some(B));
    assert_eq!(s.read(|dock| dock.active()), Some(B));
}

#[gpui::test]
fn a_tab_dropped_on_another_groups_strip_joins_it_at_the_pointer(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C, D], cx);
    s.update(|dock, _, _| {
        assert!(dock.move_panel(C, MoveTo::After(0)));
        assert!(dock.move_panel(D, MoveTo::Group { group: 1, index: 1 }));
    });
    let drawn = s.drawn();
    assert_eq!(drawn.group_tabs(0), [A, B]);
    assert_eq!(drawn.group_tabs(1), [C, D]);

    let to = between(&drawn, C);
    s.hover(A, to);
    assert_eq!(
        s.drawn().drop_hint(),
        Some(DockDrop::Insert { group: 1, index: 1 })
    );
    s.release(to);
    let drawn = s.drawn();
    assert_eq!(drawn.group_tabs(0), [B]);
    assert_eq!(drawn.group_tabs(1), [C, A, D]);
    assert_eq!(s.read(|dock| dock.shown_panels()), [B, A]);
    assert_eq!(s.read(|dock| dock.active()), Some(A));

    // On its own group's content's centre: nothing.
    let saved = s.save();
    let to = centre(&s.drawn(), 1);
    s.hover(A, to);
    assert_eq!(s.drawn().drop_hint(), None);
    s.release(to);
    assert_eq!(s.save(), saved);
    assert_eq!(s.drawn().group_tabs(1), [C, A, D]);
}

#[gpui::test]
fn a_tab_dragged_along_its_own_strip_reorders_it(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C, D], cx);
    let drawn = s.drawn();
    // Between A and B, A would stay where it is: no marker.
    s.hover(A, between(&drawn, A));
    assert_eq!(s.drawn().drop_hint(), None);
    assert_eq!(s.drawn().drop_hint_bounds(), None);
    let to = between(&drawn, C);
    s.hover_on(to);
    let hovered = s.drawn();
    assert_eq!(
        hovered.drop_hint(),
        Some(DockDrop::Insert { group: 0, index: 3 })
    );
    let marker = hovered.drop_hint_bounds().expect("drawn");
    let (c, d) = (drawn.tab_bounds(C).unwrap(), drawn.tab_bounds(D).unwrap());
    assert!(marker.left() > c.center().x && marker.right() < d.center().x);
    assert!(close(marker.center().x, d.left()), "{marker:?} {d:?}");
    s.release(to);

    assert_eq!(s.drawn().tabs(), [B, C, A, D]);
    assert_eq!(s.read(|dock| dock.active()), Some(A));
    assert_eq!(s.read(|dock| dock.group_count()), 1);
    let drawn = s.drawn();
    let lefts = [B, C, A, D].map(|id| drawn.tab_bounds(id).unwrap().left());
    assert!(lefts.is_sorted(), "{lefts:?}");

    // Kept by save and restore.
    let saved = s.save();
    assert_eq!(
        saved["groups"][0]["panels"],
        Value::from_iter(["b", "c", "a", "d"])
    );
    let fresh = s.fresh(&[A, B, C, D]);
    fresh.update(s.cx, |dock, cx| dock.dock_mut().restore(&saved, cx));
    let open = s.cx.update(|window, cx| {
        fresh.update(cx, |dock, cx| {
            for id in [A, B, C, D] {
                dock.show(id, window, cx);
            }
            dock.dock().open_panels()
        })
    });
    assert_eq!(open, [B, C, A, D]);

    // And by joining split groups.
    s.update(|dock, _, _| {
        assert!(dock.move_panel(D, MoveTo::After(0)));
        assert!(dock.move_panel(C, MoveTo::Before(0)));
        dock.join_groups();
    });
    assert_eq!(s.drawn().tabs(), [C, B, A, D]);
}

#[gpui::test]
fn dragging_a_groups_last_tab_out_merges_the_group_away(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    let before = s.drawn();
    let (tabs, title, handle) = (before.tabs(), before.title_bounds(), before.handle_bounds());
    let bounds = [A, B, C].map(|id| before.tab_bounds(id));
    let unsplit = |s: &mut Opened| {
        let drawn = s.drawn();
        assert_eq!(s.read(|dock| dock.group_count()), 1);
        assert_eq!(drawn.drawn_groups(), [0]);
        assert_eq!(drawn.tabs(), tabs);
        assert_eq!([A, B, C].map(|id| drawn.tab_bounds(id)), bounds);
        assert_eq!((drawn.title_bounds(), drawn.handle_bounds()), (title, handle));
        assert_eq!(drawn.divider_bounds(0), None);
    };

    // Dragged.
    s.drag_tab(B, right_quarter(&before, 0));
    assert_eq!(s.read(|dock| dock.group_count()), 2);
    let drawn = s.drawn();
    s.drag_tab(B, between(&drawn, A));
    unsplit(&mut s);
    assert_eq!(s.read(|dock| dock.active()), Some(B));

    // Moved.
    s.update(|dock, _, _| assert!(dock.move_panel(B, MoveTo::After(0))));
    assert_eq!(s.read(|dock| dock.group_count()), 2);
    s.update(|dock, _, _| assert!(dock.move_panel(B, MoveTo::Group { group: 0, index: 1 })));
    unsplit(&mut s);

    // Joined: every panel in one group, left to right in tab order.
    s.update(|dock, _, _| {
        assert!(dock.move_panel(C, MoveTo::Before(0)));
        assert!(dock.move_panel(A, MoveTo::After(1)));
    });
    let drawn = s.drawn();
    assert_eq!(drawn.drawn_groups(), [0, 1, 2]);
    assert_eq!(
        [0, 1, 2].map(|group| drawn.group_tabs(group)),
        [vec![C], vec![B], vec![A]]
    );
    s.update(|dock, _, _| dock.join_groups());
    assert_eq!(s.read(|dock| dock.group_count()), 1);
    assert_eq!(s.drawn().tabs(), [C, B, A]);
}

#[gpui::test]
fn a_split_is_refused_where_a_half_would_be_too_narrow(cx: &mut TestAppContext) {
    let mut s = open(&[A, B], cx);
    // A group's only open tab on its own edge.
    s.update(|dock, _, _| assert!(dock.close_tab(B)));
    let drawn = s.drawn();
    assert_eq!(drawn.tabs(), [A]);
    let to = right_quarter(&drawn, 0);
    s.hover(A, to);
    assert_eq!(s.drawn().drop_hint(), None);
    assert_eq!(s.drawn().drop_hint_bounds(), None);
    s.release(to);
    assert_eq!(s.read(|dock| dock.group_count()), 1);
    assert_eq!(s.save().get("groups"), None);

    // 600 px across: two groups of 300, whose halves are too narrow.
    s.update(|dock, window, cx| assert!(dock.show(B, window, cx)));
    s.cx.simulate_resize(size(px(600.), px(900.)));
    s.draw();
    let to = right_quarter(&s.drawn(), 0);
    s.drag_tab(B, to);
    assert_eq!(s.drawn().drawn_groups(), [0, 1]);
    let width = s.width(1);
    assert!(width / 2. < MIN_DOCK_WIDTH, "{width:?}");
    let saved = s.save();
    let to = left_quarter(&s.drawn(), 0);
    s.hover(B, to);
    assert_eq!(s.drawn().drop_hint(), None);
    assert_eq!(s.drawn().drop_hint_bounds(), None);
    s.release(to);
    assert_eq!(s.save(), saved);
    // Its centre still joins.
    let to = centre(&s.drawn(), 0);
    s.drag_tab(B, to);
    assert_eq!(s.read(|dock| dock.group_count()), 1);
    // 300 across, one group can't split either.
    s.cx.simulate_resize(size(px(300.), px(900.)));
    s.draw();
    let to = right_quarter(&s.drawn(), 0);
    s.hover(A, to);
    assert_eq!(s.drawn().drop_hint(), None);
    s.release(to);
    assert_eq!(s.read(|dock| dock.group_count()), 1);
}

#[gpui::test]
fn the_divider_moves_width_between_neighbours_and_a_double_click_evens_them(
    cx: &mut TestAppContext,
) {
    let mut s = open(&[A, B], cx);
    s.update(|dock, _, _| assert!(dock.move_panel(B, MoveTo::After(0))));
    let drawn = s.drawn();
    assert_eq!(drawn.divider_cursor(), Some(CursorStyle::ResizeLeftRight));
    let divider = drawn.divider_bounds(0).expect("drawn");
    assert_eq!(divider.size.width, px(6.));
    let (left, right) = (s.width(0), s.width(1));
    assert!(close(left, right), "{left:?} {right:?}");
    assert!(close(divider.center().x, drawn.group_bounds(1).unwrap().left()));

    let at = divider.center();
    s.drag(at, at + point(px(100.), px(0.)));
    assert!(close(s.width(0), left + px(100.)), "{:?}", s.width(0));
    assert!(close(s.width(1), right - px(100.)), "{:?}", s.width(1));
    assert!(close(s.width(0) + s.width(1), left + right));

    // Each kept at least the least.
    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.drag(at, at + point(px(2000.), px(0.)));
    assert!(close(s.width(1), MIN_DOCK_WIDTH), "{:?}", s.width(1));
    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.drag(at, point(px(0.), at.y));
    assert!(close(s.width(0), MIN_DOCK_WIDTH), "{:?}", s.width(0));

    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.double_click(at);
    assert!(close(s.width(0), s.width(1)), "{:?} {:?}", s.width(0), s.width(1));
}

#[gpui::test]
fn hiding_a_shown_panel_collapses_only_its_group(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    let whole = s.drawn().group_bounds(0).unwrap();
    s.update(|dock, _, _| assert!(dock.move_panel(C, MoveTo::After(0))));
    // Unequal widths, to come back as they were.
    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.drag(at, at + point(px(120.), px(0.)));
    let widths = (s.width(0), s.width(1));
    assert_eq!(s.read(|dock| dock.shown_panels()), [B, C]);
    assert_eq!(s.read(|dock| dock.active()), Some(C));

    // Not shown: nothing.
    assert!(!s.update(|dock, _, _| dock.hide_panel(A)));
    assert!(s.update(|dock, _, _| dock.hide_panel(C)));
    let drawn = s.drawn();
    assert!(s.read(|dock| dock.is_collapsed(1) && !dock.is_collapsed(0)));
    assert_eq!(drawn.drawn_groups(), [0]);
    assert!(close(s.width(0), whole.size.width));
    assert!(s.read(|dock| dock.is_visible()));
    assert_eq!(s.read(|dock| dock.active()), Some(B));
    assert_eq!(s.read(|dock| dock.shown_panels()), [B]);
    assert_eq!(drawn.tabs(), [A, B]);
    // Its tab is kept, its group's.
    assert_eq!(s.read(|dock| dock.open_panels()), [A, B, C]);

    s.update(|dock, window, cx| assert!(dock.show(C, window, cx)));
    let drawn = s.drawn();
    assert_eq!(drawn.drawn_groups(), [0, 1]);
    assert_eq!(drawn.group_tabs(1), [C]);
    assert_eq!(s.read(|dock| dock.shown_panels()), [B, C]);
    assert!(close(s.width(0), widths.0) && close(s.width(1), widths.1));

    // The only group drawn: the dock hides.
    s.update(|dock, _, _| assert!(dock.hide_panel(C)));
    s.update(|dock, _, _| assert!(dock.hide_panel(B)));
    assert!(!s.read(|dock| dock.is_visible()));
    s.update(|dock, window, cx| assert!(dock.show(C, window, cx)));
    assert_eq!(s.drawn().drawn_groups(), [0, 1]);

    // The dock's — hides it all.
    let hide = s.cx.debug_bounds("icon-button:dock-hide").expect("drawn");
    let groups = s.drawn();
    let last = groups.group_bounds(1).unwrap();
    assert!(last.left() < hide.left() && hide.right() <= last.right());
    s.cx.simulate_click(hide.center(), Modifiers::none());
    s.draw();
    assert!(!s.read(|dock| dock.is_visible()));
    assert_eq!(s.drawn().drawn_groups(), Vec::<usize>::new());
}

#[gpui::test]
fn closing_a_groups_last_open_tab_hides_the_group_but_keeps_its_place(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    s.update(|dock, _, _| assert!(dock.move_panel(C, MoveTo::After(0))));
    s.focus(C);
    s.draw();
    assert!(!s.hidden_panel_focused());

    s.update(|dock, _, _| assert!(dock.close_tab(C)));
    let drawn = s.drawn();
    assert_eq!(drawn.drawn_groups(), [0]);
    assert_eq!(s.read(|dock| (dock.group_count(), dock.group_of(C))), (2, Some(1)));
    assert!(s.read(|dock| dock.is_visible()));
    assert_eq!(s.read(|dock| dock.active()), Some(B));
    assert!(s.hidden_panel_focused());

    s.update(|dock, window, cx| assert!(dock.show(C, window, cx)));
    let drawn = s.drawn();
    assert_eq!(drawn.drawn_groups(), [0, 1]);
    assert_eq!(drawn.group_tabs(1), [C]);
    assert!(!s.hidden_panel_focused());

    // Every tab closed: the dock hides, as without groups.
    s.update(|dock, _, _| {
        for id in [C, A, B] {
            assert!(dock.close_tab(id));
        }
    });
    assert!(!s.read(|dock| dock.is_visible()));
    assert!(s.drawn().tabs().is_empty());

    // Shown in a drawn group that isn't the focused one, it isn't hidden.
    s.update(|dock, window, cx| {
        for id in [C, A] {
            assert!(dock.show(id, window, cx));
        }
    });
    assert_eq!(s.drawn().drawn_groups(), [0, 1]);
    assert_eq!(s.read(|dock| dock.active()), Some(A));
    s.focus(A);
    s.draw();
    assert_eq!(s.read(|dock| dock.active()), Some(A));
    s.focus(C);
    assert!(!s.hidden_panel_focused());
    // Focus moving into it focuses its group.
    s.draw();
    assert_eq!(s.read(|dock| dock.active()), Some(C));
}

#[gpui::test]
fn groups_are_saved_and_restored(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C, D], cx);
    // A default dock saves what a dock saved before groups.
    let default = s.save();
    let expected: Value = serde_json::from_str(
        r#"{"visible": true, "active": "d", "joined": ["a", "b", "c", "d"],
            "size": 300.0, "side_size": 300.0, "bottom_size": 300.0}"#,
    )
    .unwrap();
    assert_eq!(default, expected);

    // Reordered, split and collapsed.
    s.update(|dock, _, _| {
        assert!(dock.move_panel(D, MoveTo::Group { group: 0, index: 0 }));
        assert!(dock.move_panel(C, MoveTo::After(0)));
        assert!(dock.hide_panel(C));
    });
    let saved = s.save();
    let groups: Value = serde_json::from_str(
        r#"[{"panels": ["d", "a", "b"], "active": "d", "weight": 0.5, "collapsed": false},
            {"panels": ["c"], "active": "c", "weight": 0.5, "collapsed": true}]"#,
    )
    .unwrap();
    assert_eq!(saved["groups"], groups);
    assert_eq!(saved["active"], Value::from("d"));

    let fresh = s.fresh(&[A, B, C, D]);
    fresh.update(s.cx, |dock, cx| dock.dock_mut().restore(&saved, cx));
    fresh.read_with(s.cx, |dock, cx| {
        let dock = dock.dock();
        assert_eq!(dock.save(cx), saved);
        assert_eq!(dock.group_count(), 2);
        assert!(dock.is_collapsed(1));
        assert_eq!(dock.shown_panels(), [D]);
        assert_eq!(dock.open_panels(), [D, C]);
    });

    // Unknown ids and empty groups go; a panel it doesn't place goes in
    // the first group.
    let odd: Value = serde_json::from_str(
        r#"{"visible": true, "active": "a", "groups": [
            {"panels": ["ghost"], "active": "ghost"},
            {"panels": ["b", "ghost", "a", "b"], "active": "a", "weight": 2.0},
            {"panels": []},
            {"panels": ["c"], "active": "c", "weight": 1.0, "collapsed": false}]}"#,
    )
    .unwrap();
    let fresh = s.fresh(&[A, B, C, D]);
    fresh.update(s.cx, |dock, cx| dock.dock_mut().restore(&odd, cx));
    fresh.read_with(s.cx, |dock, cx| {
        let dock = dock.dock();
        assert_eq!(dock.group_count(), 2);
        assert_eq!(dock.group_of(D), Some(0));
        assert_eq!(dock.shown_panels(), [A, C]);
        assert_eq!(dock.active(), Some(A));
        let groups: Value = serde_json::from_str(
            r#"[{"panels": ["b", "a", "d"], "active": "a", "weight": 2.0, "collapsed": false},
                {"panels": ["c"], "active": "c", "weight": 1.0, "collapsed": false}]"#,
        )
        .unwrap();
        assert_eq!(dock.save(cx)["groups"], groups);
    });

    // Saved without groups: one, in the order added.
    let split = s.fresh(&[A, B, C, D]);
    split.update(s.cx, |dock, cx| {
        assert!(dock.dock_mut().move_panel(A, MoveTo::After(0)));
        dock.dock_mut().restore(&default, cx);
    });
    split.read_with(s.cx, |dock, cx| {
        let dock = dock.dock();
        assert_eq!(dock.group_count(), 1);
        assert_eq!(dock.group_of(A), Some(0));
        assert_eq!(dock.save(cx), default);
    });
}

#[gpui::test]
fn a_side_dock_stacks_its_groups_top_to_bottom(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    s.update(|dock, _, _| assert!(dock.move_panel(C, MoveTo::After(0))));
    s.move_to(DockPosition::Left);
    let drawn = s.drawn();
    assert_eq!(drawn.drawn_groups(), [0, 1]);
    let (top, bottom) = (drawn.group_bounds(0).unwrap(), drawn.group_bounds(1).unwrap());
    assert!(top.bottom() <= bottom.top(), "{top:?} {bottom:?}");
    assert_eq!(top.left(), bottom.left());
    assert_eq!(top.size.width, bottom.size.width);
    assert!(close(top.size.height, bottom.size.height));

    // Its zones are the top and bottom quarters; the left one joins.
    let content = drawn.group_content_bounds(1).unwrap();
    let up = point(content.center().x, content.top() + px(10.));
    s.hover(A, up);
    assert_eq!(s.drawn().drop_hint(), Some(DockDrop::SplitBefore(1)));
    let hint = s.drawn().drop_hint_bounds().unwrap();
    assert!(close(hint.top(), bottom.top()) && close(hint.size.height, bottom.size.height / 2.));
    assert!(close(hint.size.width, bottom.size.width));
    let down = point(content.center().x, content.bottom() - px(10.));
    s.hover_on(down);
    assert_eq!(s.drawn().drop_hint(), Some(DockDrop::SplitAfter(1)));
    let side = point(content.left() + px(10.), content.center().y);
    s.hover_on(side);
    assert_eq!(s.drawn().drop_hint(), Some(DockDrop::Join(1)));
    s.release(side);
    assert_eq!(s.drawn().group_tabs(1), [C, A]);

    // Its border drags up and down.
    let drawn = s.drawn();
    assert_eq!(drawn.divider_cursor(), Some(CursorStyle::ResizeUpDown));
    let divider = drawn.divider_bounds(0).unwrap();
    assert_eq!(divider.size.height, px(6.));
    let (above, below) = (s.height(0), s.height(1));
    let at = divider.center();
    s.drag(at, at + point(px(0.), px(60.)));
    assert!(close(s.height(0), above + px(60.)), "{:?}", s.height(0));
    assert!(close(s.height(1), below - px(60.)), "{:?}", s.height(1));
    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.drag(at, at + point(px(0.), px(5000.)));
    assert!(close(s.height(1), MIN_DOCK_HEIGHT), "{:?}", s.height(1));
}

#[gpui::test]
fn groups_and_the_divider_are_named_for_assistive_tech(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    s.update(|dock, _, _| assert!(dock.move_panel(C, MoveTo::After(0))));
    s.cx.update(|_, cx| kiln_ui::clear_nodes(cx));
    s.update(|_, _, _| ());
    let nodes = s.cx.update(|window, cx| kiln_ui::nodes(window, cx));
    let labels = |role: Role| -> Vec<String> {
        (nodes.iter())
            .filter(|node| node.role == role)
            .map(|node| node.label.to_string())
            .collect()
    };
    assert_eq!(labels(Role::Group), ["The b", "The c"]);
    assert_eq!(labels(Role::TabList).len(), 2);
    assert_eq!(labels(Role::Tab).len(), 3);
    assert_eq!(labels(Role::Splitter), ["Resize panel groups"]);
    assert_eq!(labels(Role::Region), ["Panel"]);
}

#[gpui::test]
fn repeated_splits_and_a_hidden_or_closed_group_still_fill_the_dock(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    let whole = s.drawn().group_bounds(0).unwrap().size.width;
    let filled = |s: &mut Opened| {
        let drawn = s.drawn();
        let groups = drawn.drawn_groups();
        let width: Pixels = (groups.iter()).map(|&group| drawn.group_bounds(group).unwrap().size.width).sum();
        assert!(close(width, whole), "{groups:?}: {width:?} of {whole:?}");
    };
    // Split, merged back and split again: each split halves a weight.
    s.update(|dock, _, _| {
        assert!(dock.move_panel(B, MoveTo::After(0)));
        assert!(dock.move_panel(B, MoveTo::Group { group: 0, index: 1 }));
        assert!(dock.move_panel(B, MoveTo::After(0)));
    });
    filled(&mut s);
    assert!(close(s.width(0), s.width(1)));

    // One of three hidden, then shown, then closed.
    s.update(|dock, _, _| assert!(dock.move_panel(A, MoveTo::After(1))));
    assert_eq!(s.drawn().drawn_groups(), [0, 1, 2]);
    filled(&mut s);
    s.update(|dock, _, _| assert!(dock.hide_panel(B)));
    assert_eq!(s.drawn().drawn_groups(), [0, 2]);
    filled(&mut s);
    // The divider between the two left drags by the pointer.
    let (left, right) = (s.width(0), s.width(2));
    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.drag(at, at + point(px(80.), px(0.)));
    assert!(close(s.width(0), left + px(80.)), "{:?}", s.width(0));
    assert!(close(s.width(2), right - px(80.)), "{:?}", s.width(2));
    filled(&mut s);
    s.update(|dock, window, cx| assert!(dock.show(B, window, cx)));
    assert_eq!(s.drawn().drawn_groups(), [0, 1, 2]);
    filled(&mut s);
    s.update(|dock, _, _| assert!(dock.close_tab(A)));
    assert_eq!(s.drawn().drawn_groups(), [0, 1]);
    filled(&mut s);
    let (left, right) = (s.width(0), s.width(1));
    let at = s.drawn().divider_bounds(0).unwrap().center();
    s.drag(at, at - point(px(60.), px(0.)));
    assert!(close(s.width(0), left - px(60.)), "{:?}", s.width(0));
    assert!(close(s.width(1), right + px(60.)), "{:?}", s.width(1));
    filled(&mut s);
}

#[gpui::test]
fn restoring_keeps_a_collapsed_group_holding_the_fresh_docks_active_panel(
    cx: &mut TestAppContext,
) {
    let mut s = open(&[A, B, C], cx);
    s.update(|dock, _, _| {
        assert!(dock.move_panel(C, MoveTo::After(0)));
        assert!(dock.hide_panel(B));
    });
    assert!(s.read(|dock| dock.is_collapsed(0)));
    assert_eq!(s.read(|dock| dock.shown_panels()), [C]);
    let saved = s.save();

    // A fresh dock's active panel is A, its first, in the collapsed group.
    let fresh = s.fresh(&[A, B, C]);
    assert_eq!(fresh.read_with(s.cx, |dock, _| dock.dock().active()), Some(A));
    fresh.update(s.cx, |dock, cx| dock.dock_mut().restore(&saved, cx));
    fresh.read_with(s.cx, |dock, cx| {
        let dock = dock.dock();
        assert!(dock.is_collapsed(0));
        assert_eq!(dock.shown_panels(), [C]);
        assert_eq!(dock.active(), Some(C));
        assert_eq!(dock.save(cx), saved);
    });
}

#[gpui::test]
fn a_drop_that_hides_the_focused_panel_tells_the_host(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    s.focus(C);
    s.draw();
    assert!(!s.hidden_panel_focused());
    // A, not shown, dragged after C: it shows in C's place.
    let to = between(&s.drawn(), C);
    s.hover(A, to);
    assert_eq!(
        s.drawn().drop_hint(),
        Some(DockDrop::Insert { group: 0, index: 3 })
    );
    s.release(to);
    assert_eq!(s.drawn().tabs(), [B, C, A]);
    assert_eq!(s.read(|dock| dock.active()), Some(A));
    assert!(s.hidden_panel_focused());
    assert_eq!(s.events(), [DockEvent::HidFocus]);
}

#[gpui::test]
fn focus_moving_between_shown_groups_notifies_observers(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    s.update(|dock, _, _| assert!(dock.move_panel(C, MoveTo::After(0))));
    s.focus(C);
    s.draw();
    assert_eq!(s.read(|dock| dock.active()), Some(C));
    // Focus moves to B, shown in the other group, by the host alone: the
    // dock hears it as it draws, and tells its observers.
    let before = s.notified();
    s.focus(B);
    s.draw();
    assert_eq!(s.read(|dock| dock.active()), Some(B));
    assert!(s.notified() > before, "the change of active panel went unheard");
}

#[gpui::test]
fn a_single_drawn_group_is_named_for_assistive_tech(cx: &mut TestAppContext) {
    let mut s = open(&[A, B, C], cx);
    // Built with one group, at the bottom.
    assert_eq!(s.labels(Role::Group), ["The c"]);
    assert_eq!(s.labels(Role::Region), ["Panel"]);
    // Split, then back to one by hiding the other.
    s.update(|dock, _, _| assert!(dock.move_panel(B, MoveTo::After(0))));
    assert_eq!(s.labels(Role::Group), ["The c", "The b"]);
    s.update(|dock, _, _| assert!(dock.hide_panel(C)));
    assert_eq!(s.drawn().drawn_groups(), [1]);
    assert_eq!(s.labels(Role::Group), ["The b"]);
    // By closing the other's last tab.
    s.update(|dock, window, cx| assert!(dock.show(C, window, cx)));
    s.update(|dock, _, _| assert!(dock.close_tab(B)));
    assert_eq!(s.labels(Role::Group), ["The c"]);
    // At a side, in its column's landmark.
    s.move_to(DockPosition::Left);
    assert_eq!(s.labels(Role::Group), ["The c"]);
    assert_eq!(s.labels(Role::Complementary), ["The c"]);
}
