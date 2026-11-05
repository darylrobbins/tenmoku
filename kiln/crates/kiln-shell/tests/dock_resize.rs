// SPDX-License-Identifier: Apache-2.0

//! A dock's inner edge drags its size, showing the resize cursor, between
//! the dock's minimum and what leaves the centre its own, beside the other
//! docks and the window's other rows; pressed twice it puts back the size
//! built with; a window too small, or a row appearing, draws it smaller
//! without forgetting it, from the first frame, the docks sharing the room
//! within their minimums; a dock replaced leaves nothing behind; and the
//! size is saved and restored.

use gpui::{
    App, AppContext as _, Context, CursorStyle, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, point, prelude::FluentBuilder as _, px, size,
};
use kiln_command::Value;
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId,
    dock::{MIN_DOCK_HEIGHT, MIN_DOCK_WIDTH, track_centre},
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};
use kiln_ui::{Icon, RAIL_WIDTH};

/// The size every dock is built with.
const BUILT: Pixels = px(200.);
/// The height of the row above the centre, as a title bar's.
const TOP_ROW: Pixels = px(30.);

struct Panel(FocusHandle);

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.0.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        PanelId("panel")
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        "Panel".into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        Icon::Files
    }
}

/// A centre with docks around it, the side docks either side of it, the
/// bottom one below, under a row as a title bar is, if it shows. The
/// centre keeps its least, as the shell's does.
struct Host {
    docks: Vec<(DockPosition, Entity<Dock>)>,
    top_row: bool,
}

impl Host {
    fn at(&self, position: DockPosition) -> Option<Entity<Dock>> {
        (self.docks.iter())
            .find(|(at, _)| *at == position)
            .map(|(_, dock)| dock.clone())
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let centre = div()
            .debug_selector(|| "centre".into())
            .relative()
            .flex_1()
            .min_w(MIN_PANE_WIDTH)
            .h_full()
            .child(track_centre());
        div()
            .size_full()
            .flex()
            .flex_col()
            .when(self.top_row, |host| {
                host.child(div().flex_none().w_full().h(TOP_ROW))
            })
            .child(
                div()
                    .flex_1()
                    .min_h(MIN_PANE_HEIGHT)
                    .w_full()
                    .flex()
                    .children(self.at(DockPosition::Left))
                    .child(centre)
                    .children(self.at(DockPosition::Right)),
            )
            .children(self.at(DockPosition::Bottom))
    }
}

/// A dock at `position` built `BUILT` across, then restored `saved` if
/// given.
fn new_dock(position: DockPosition, saved: Option<&Value>, cx: &mut App) -> Entity<Dock> {
    cx.new(|cx| {
        let mut state = DockState::new(position).size(BUILT);
        if let Some(saved) = saved {
            state.restore(saved, cx);
        }
        let mut dock = Dock::with_state(state);
        let panel = cx.new(|cx| Panel(cx.focus_handle()));
        dock.add_panel(panel, cx);
        dock
    })
}

struct Opened<'a> {
    host: Entity<Host>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    /// Draws a frame.
    fn frame(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// Draws a frame, then those it asks for.
    fn draw(&mut self) {
        self.frame();
        for _ in 0..4 {
            if self.cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
                return;
            }
            self.frame();
        }
        panic!("the frames don't settle");
    }

    /// The most the dock's size can be.
    fn max_size(&mut self, position: DockPosition) -> Pixels {
        let dock = self.dock(position);
        self.cx
            .update(|window, cx| dock.read(cx).dock().max_size(window, cx))
    }

    /// The centre's extent along `position`'s axis.
    fn centre_along(&mut self, position: DockPosition) -> Pixels {
        let centre = self.centre();
        match position {
            DockPosition::Bottom => centre.size.height,
            DockPosition::Left | DockPosition::Right => centre.size.width,
        }
    }

    fn dock(&mut self, position: DockPosition) -> Entity<Dock> {
        let dock = self.host.read_with(self.cx, |host, _| host.at(position));
        dock.expect("a dock there")
    }

    fn size(&mut self, position: DockPosition) -> Pixels {
        let dock = self.dock(position);
        dock.read_with(self.cx, |dock, _| dock.dock().current_size())
    }

    /// The dock's column as drawn: its width, or its height at the
    /// bottom.
    fn drawn(&mut self, position: DockPosition) -> Pixels {
        let dock = self.dock(position);
        let column = dock.read_with(self.cx, |dock, _| dock.dock().drawn().column_bounds());
        let column = column.expect("shown");
        match position {
            DockPosition::Bottom => column.size.height,
            DockPosition::Left | DockPosition::Right => column.size.width,
        }
    }

    /// The middle of the dock's edge handle.
    fn handle(&mut self, position: DockPosition) -> Point<Pixels> {
        let dock = self.dock(position);
        let handle = dock.read_with(self.cx, |dock, _| dock.dock().drawn().handle_bounds());
        handle.expect("shown").center()
    }

    fn cursor(&mut self, position: DockPosition) -> Option<CursorStyle> {
        let dock = self.dock(position);
        dock.read_with(self.cx, |dock, _| dock.dock().drawn().handle_cursor())
    }

    /// Drags `position`'s handle by `by`.
    fn drag(&mut self, position: DockPosition, by: Point<Pixels>) {
        let at = self.handle(position);
        let to = at + by;
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(at, MouseButton::Left, none);
        // Past the drag's threshold, then to the end.
        let nudge = point(px(3.), px(3.));
        self.cx
            .simulate_mouse_move(at + nudge, MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_up(to, MouseButton::Left, none);
        self.draw();
    }

    fn double_click(&mut self, position: DockPosition) {
        let at = self.handle(position);
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

    /// Clicks `position`'s handle, then presses it again and drags by
    /// `by` before letting go.
    fn double_click_drag(&mut self, position: DockPosition, by: Point<Pixels>) {
        let at = self.handle(position);
        let none = Modifiers::none();
        self.cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: none,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: none,
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: none,
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        let to = at + by;
        self.cx
            .simulate_mouse_move(at + by * 0.5, MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_up(to, MouseButton::Left, none);
        self.draw();
    }

    fn centre(&mut self) -> gpui::Bounds<Pixels> {
        self.cx.debug_bounds("centre").expect("drawn")
    }

    fn viewport(&mut self) -> gpui::Size<Pixels> {
        self.cx.update(|window, _| window.viewport_size())
    }
}

/// A window with docks at `positions`, each built `BUILT` across, drawn.
fn open<'a>(cx: &'a mut TestAppContext, positions: &[DockPosition]) -> Opened<'a> {
    let mut opened = build(cx, positions, None);
    opened.draw();
    opened
}

/// A window with docks at `positions`, each built `BUILT` across, then
/// restored `saved` if given, not yet drawn.
fn build<'a>(
    cx: &'a mut TestAppContext,
    positions: &[DockPosition],
    saved: Option<Value>,
) -> Opened<'a> {
    let positions = positions.to_vec();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let docks = (positions.iter())
            .map(|&position| (position, new_dock(position, saved.as_ref(), cx)))
            .collect();
        Host {
            docks,
            top_row: true,
        }
    });
    Opened { host, cx }
}

#[gpui::test]
fn an_inner_edge_drags_the_size(cx: &mut TestAppContext) {
    use DockPosition::*;
    let mut s = open(cx, &[Left, Right, Bottom]);
    for position in [Left, Right, Bottom] {
        assert_eq!(s.drawn(position), BUILT, "{position:?}");
    }
    assert_eq!(s.cursor(Left), Some(CursorStyle::ResizeLeftRight));
    assert_eq!(s.cursor(Right), Some(CursorStyle::ResizeLeftRight));
    assert_eq!(s.cursor(Bottom), Some(CursorStyle::ResizeUpDown));

    s.drag(Right, point(px(-100.), px(0.)));
    assert_eq!(s.size(Right), BUILT + px(100.));
    assert_eq!(s.drawn(Right), BUILT + px(100.));
    s.drag(Left, point(px(100.), px(0.)));
    assert_eq!(s.size(Left), BUILT + px(100.));
    assert_eq!(s.drawn(Left), BUILT + px(100.));
    s.drag(Bottom, point(px(0.), px(-50.)));
    assert_eq!(s.size(Bottom), BUILT + px(50.));
    assert_eq!(s.drawn(Bottom), BUILT + px(50.));
}

#[gpui::test]
fn a_drag_stops_at_the_limits_and_a_double_click_puts_back_the_size(cx: &mut TestAppContext) {
    use DockPosition::*;
    // All three docks, under the top row: each leaves the centre its least
    // beside the others and the row.
    let mut s = open(cx, &[Left, Right, Bottom]);
    let far = px(10_000.);
    for position in [Right, Left, Bottom] {
        // Towards the window's edge, then towards the centre.
        let (out, into) = match position {
            Left => (point(-far, px(0.)), point(far, px(0.))),
            Right => (point(far, px(0.)), point(-far, px(0.))),
            Bottom => (point(px(0.), far), point(px(0.), -far)),
        };
        let (min, least) = match position {
            Bottom => (MIN_DOCK_HEIGHT, MIN_PANE_HEIGHT),
            Left | Right => (MIN_DOCK_WIDTH, MIN_PANE_WIDTH),
        };
        s.drag(position, out);
        assert_eq!(s.size(position), min, "{position:?}");
        assert_eq!(s.drawn(position), min, "{position:?}");

        s.drag(position, into);
        assert_eq!(s.centre_along(position), least, "{position:?}");
        let most = s.size(position);
        assert_eq!(s.drawn(position), most, "{position:?}");
        // What a command growing it stops at.
        assert_eq!(s.max_size(position), most, "{position:?}");

        // A window too small for it draws it smaller, but keeps it.
        let viewport = s.viewport();
        s.cx.simulate_resize(size(viewport.width - px(50.), viewport.height - px(50.)));
        s.draw();
        assert_eq!(s.size(position), most, "{position:?}");
        assert!(s.drawn(position) < most, "{position:?}");
        let centre = s.centre_along(position);
        assert!((centre - least).abs() < px(1.), "{position:?}: {centre:?}");
        s.cx.simulate_resize(viewport);
        s.draw();
        assert_eq!(s.drawn(position), most, "{position:?}");

        s.double_click(position);
        assert_eq!(s.size(position), BUILT, "{position:?}");
        assert_eq!(s.drawn(position), BUILT, "{position:?}");
    }
}

#[gpui::test]
fn a_drag_on_the_second_click_starts_from_the_size_put_back(cx: &mut TestAppContext) {
    use DockPosition::*;
    let mut s = open(cx, &[Left, Right, Bottom]);
    for position in [Left, Right, Bottom] {
        // A step towards the centre.
        let wider = |by: f32| match position {
            Left => point(px(by), px(0.)),
            Right => point(px(-by), px(0.)),
            Bottom => point(px(0.), px(-by)),
        };
        s.drag(position, wider(100.));
        assert_eq!(s.size(position), BUILT + px(100.), "{position:?}");
        s.double_click_drag(position, wider(10.));
        assert_eq!(s.size(position), BUILT + px(10.), "{position:?}");
        assert_eq!(s.drawn(position), BUILT + px(10.), "{position:?}");
    }
}

#[gpui::test]
fn restored_sizes_too_big_are_drawn_within_the_window_from_the_first_frame(
    cx: &mut TestAppContext,
) {
    use DockPosition::*;
    let saved = Value::from_iter([("size", Value::from(5_000.))]);
    let mut s = build(cx, &[Left, Right, Bottom], Some(saved));
    s.frame();
    let viewport = s.viewport();
    for position in [Left, Right, Bottom] {
        assert_eq!(s.size(position), px(5_000.), "{position:?}");
        let dock = s.dock(position);
        let rail = dock.read_with(s.cx, |dock, _| dock.dock().drawn().rail_bounds());
        let rail = rail.expect("drawn");
        assert!(rail.left() >= px(0.), "{position:?}: {rail:?}");
        assert!(rail.right() <= viewport.width, "{position:?}: {rail:?}");
        assert!(rail.bottom() <= viewport.height, "{position:?}: {rail:?}");
    }
    assert_eq!(s.centre_along(Right), MIN_PANE_WIDTH);
    assert_eq!(s.centre_along(Bottom), MIN_PANE_HEIGHT);
    // The side docks share the room alike.
    assert_eq!(s.drawn(Left), s.drawn(Right));
}

#[gpui::test]
fn a_dock_at_its_least_leaves_the_rest_of_the_room_to_the_other(cx: &mut TestAppContext) {
    use DockPosition::*;
    let mut s = open(cx, &[Left, Right]);
    s.drag(Right, point(px(-10_000.), px(0.)));
    assert_eq!(s.centre_along(Right), MIN_PANE_WIDTH);
    let most = s.size(Right);
    // Room for the left dock's least and 250 px: shared in proportion,
    // the left's share would fall below its least.
    let viewport = s.viewport();
    let narrow = MIN_DOCK_WIDTH + px(250.) + RAIL_WIDTH * 2. + MIN_PANE_WIDTH;
    s.cx.simulate_resize(size(narrow, viewport.height));
    s.draw();
    assert_eq!(s.drawn(Left), MIN_DOCK_WIDTH);
    assert_eq!(s.drawn(Right), px(250.));
    assert_eq!(s.centre_along(Right), MIN_PANE_WIDTH);
    assert_eq!((s.size(Left), s.size(Right)), (BUILT, most));
}

#[gpui::test]
fn a_row_appearing_draws_a_dock_at_its_most_smaller(cx: &mut TestAppContext) {
    use DockPosition::*;
    let mut s = open(cx, &[Bottom]);
    s.host.update(s.cx, |host, cx| {
        host.top_row = false;
        cx.notify();
    });
    s.draw();
    s.drag(Bottom, point(px(0.), px(-10_000.)));
    assert_eq!(s.centre_along(Bottom), MIN_PANE_HEIGHT);
    let most = s.size(Bottom);
    assert_eq!(s.drawn(Bottom), most);

    // The viewport as it was: the row takes its height from the dock.
    s.host.update(s.cx, |host, cx| {
        host.top_row = true;
        cx.notify();
    });
    s.draw();
    assert_eq!(s.centre_along(Bottom), MIN_PANE_HEIGHT);
    assert_eq!(s.drawn(Bottom), most - TOP_ROW);
    assert_eq!(s.size(Bottom), most);
    assert_eq!(s.max_size(Bottom), most - TOP_ROW);
    s.host.update(s.cx, |host, cx| {
        host.top_row = false;
        cx.notify();
    });
    s.draw();
    assert_eq!(s.drawn(Bottom), most);
}

#[gpui::test]
fn a_dock_replaced_while_kept_leaves_nothing_behind(cx: &mut TestAppContext) {
    use DockPosition::*;
    let saved = Value::from_iter([("size", Value::from(5_000.))]);
    let mut s = build(cx, &[Right], Some(saved));
    s.draw();
    let old = s.dock(Right);
    let new = s.cx.update(|_, cx| new_dock(Right, None, cx));
    s.host.update(s.cx, |host, cx| {
        host.docks = vec![(Right, new)];
        cx.notify();
    });
    s.draw();
    assert_eq!(s.drawn(Right), BUILT);
    s.drag(Right, point(px(-10_000.), px(0.)));
    assert_eq!(s.centre_along(Right), MIN_PANE_WIDTH);
    assert!(s.size(Right) > BUILT);
    old.read_with(s.cx, |old, _| {
        assert_eq!(old.dock().current_size(), px(5_000.))
    });
}

#[gpui::test]
fn the_size_is_saved_and_restored_at_least_the_least(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut dock = DockState::new(DockPosition::Right).size(BUILT);
        dock.set_size(px(260.));
        let saved = dock.save(cx);
        assert_eq!(saved.get("size").and_then(Value::as_f64), Some(260.));

        let mut restored = |position, saved: &Value| {
            let mut dock = DockState::new(position).size(BUILT);
            dock.restore(saved, cx);
            dock.current_size()
        };
        assert_eq!(restored(DockPosition::Right, &saved), px(260.));
        let small = Value::from_iter([("size", Value::from(20.))]);
        assert_eq!(restored(DockPosition::Right, &small), MIN_DOCK_WIDTH);
        assert_eq!(restored(DockPosition::Bottom, &small), MIN_DOCK_HEIGHT);
        let none = Value::from_iter([("visible", Value::from(true))]);
        assert_eq!(restored(DockPosition::Right, &none), BUILT);
    });
}
