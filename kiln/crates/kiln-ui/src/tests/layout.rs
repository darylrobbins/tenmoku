// SPDX-License-Identifier: LicenseRef-Proprietary

//! ScrollArea, SplitPane and the flex shorthands: a scroll area's thumb
//! follows and drives its scrolling, and dragging a split pane's border
//! changes its ratio within its sides' minimums.

use gpui::{
    AppContext as _, Axis, Bounds, Context, Entity, InteractiveElement as _, IntoElement,
    Modifiers, MouseButton, ParentElement as _, Pixels, Point, Render, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, Styled as _, Subscription, TestAppContext, TouchPhase, VisualTestContext,
    Window, div, point, px, size,
};

use crate::{
    SCROLLBAR_WIDTH, ScrollArea, SplitPane, SplitPaneEvent, h_flex,
    scroll_area::{scroll_for_thumb, thumb_bounds},
    v_flex,
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

/// Presses at `from`, moves past GPUI's drag threshold, then to `to`, and
/// releases.
fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(
        point(from.x + px(3.), from.y + px(3.)),
        MouseButton::Left,
        none,
    );
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(cx);
}

#[test]
fn the_thumb_is_the_viewports_share_of_the_track() {
    let track = Bounds::new(point(px(0.), px(10.)), size(SCROLLBAR_WIDTH, px(100.)));
    assert_eq!(
        thumb_bounds(track, px(0.), px(0.)),
        None,
        "content that fits"
    );

    // A 100 px viewport on 400 px of content: a quarter.
    let top = thumb_bounds(track, px(0.), px(300.)).unwrap();
    assert_eq!((top.top(), top.size.height), (px(10.), px(25.)));
    let half = thumb_bounds(track, px(150.), px(300.)).unwrap();
    assert_eq!(half.top(), px(10.) + px(37.5));
    let end = thumb_bounds(track, px(300.), px(300.)).unwrap();
    assert_eq!(end.bottom(), track.bottom());

    let long = thumb_bounds(track, px(0.), px(100_000.)).unwrap();
    assert_eq!(long.size.height, px(20.), "never shorter than 20 px");

    assert_eq!(
        scroll_for_thumb(track, px(25.), half.top(), px(300.)),
        px(150.)
    );
    assert_eq!(
        scroll_for_thumb(track, px(25.), px(-50.), px(300.)),
        px(0.),
        "clamped"
    );
}

/// A scroll area 200 by 100 holding twenty rows 20 px tall.
struct Scrolled {
    handle: ScrollHandle,
}

impl Render for Scrolled {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            ScrollArea::new("area")
                .track_scroll(&self.handle)
                .debug_selector(|| "area".into())
                .w(px(200.))
                .h(px(100.))
                .children((0..20).map(|ix| {
                    div()
                        .h(px(20.))
                        .debug_selector(move || format!("row-{ix}"))
                        .child(format!("Row {ix}"))
                })),
        )
    }
}

#[gpui::test]
fn scroll_area_scrolls_and_its_thumb_drags(cx: &mut TestAppContext) {
    let handle = ScrollHandle::new();
    let given = handle.clone();
    let (_, cx) = cx.add_window_view(|_, _| Scrolled { handle: given });
    draw(cx);

    let area = cx.debug_bounds("area").unwrap();
    assert_eq!(area.size, size(px(200.), px(100.)), "the caller's size");
    assert_eq!(handle.max_offset().y, px(300.));

    cx.simulate_event(ScrollWheelEvent {
        position: area.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-50.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    assert_eq!(handle.offset().y, px(-50.));
    let row = cx.debug_bounds("row-5").unwrap();
    assert_eq!(row.top(), area.top() + px(50.), "row 5 moved up 50 px");

    // Drag the thumb down half its room: half the content's.
    handle.set_offset(point(px(0.), px(0.)));
    draw(cx);
    let track = Bounds::new(
        point(area.right() - SCROLLBAR_WIDTH, area.top()),
        size(SCROLLBAR_WIDTH, area.size.height),
    );
    let thumb = thumb_bounds(track, px(0.), px(300.)).unwrap();
    let from = thumb.center();
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(point(from.x, from.y + px(37.5)), MouseButton::Left, none);
    cx.simulate_mouse_up(point(from.x, from.y + px(37.5)), MouseButton::Left, none);
    draw(cx);
    assert!(near(handle.offset().y, px(-150.)), "{:?}", handle.offset());

    // Moving after the release scrolls nothing.
    cx.simulate_mouse_move(point(from.x, from.y + px(70.)), None, none);
    draw(cx);
    assert!(near(handle.offset().y, px(-150.)));
}

/// A side of a split pane, filling it.
struct Side(&'static str);

impl Render for Side {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let name = self.0;
        div().size_full().debug_selector(move || name.into())
    }
}

/// A split pane 601 px wide (300 a side and the border) or tall, and its
/// events.
struct Split {
    pane: Entity<SplitPane>,
    resized: Vec<f32>,
    _events: Subscription,
}

fn split(axis: Axis, cx: &mut TestAppContext) -> (Entity<Split>, &mut VisualTestContext) {
    let (split, cx) = cx.add_window_view(|window, cx| {
        let first = cx.new(|_| Side("first"));
        let second = cx.new(|_| Side("second"));
        let pane =
            cx.new(|_| SplitPane::new(axis, first, second).with_min_sizes(px(100.), px(150.)));
        let events = cx.subscribe_in(&pane, window, |split: &mut Split, _, event, _, _| {
            let SplitPaneEvent::Resized(ratio) = event;
            split.resized.push(*ratio);
        });
        Split {
            pane,
            resized: Vec::new(),
            _events: events,
        }
    });
    draw(cx);
    (split, cx)
}

impl Render for Split {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().size(px(601.)).child(self.pane.clone()))
    }
}

fn ratio(split: &Entity<Split>, cx: &mut VisualTestContext) -> f32 {
    split.read_with(cx, |split, cx| split.pane.read(cx).ratio())
}

#[gpui::test]
fn dragging_the_border_changes_the_ratio_within_the_minimums(cx: &mut TestAppContext) {
    let (split, cx) = split(Axis::Horizontal, cx);
    let first = cx.debug_bounds("first").unwrap();
    let second = cx.debug_bounds("second").unwrap();
    assert!(near(first.size.width, px(300.)), "{first:?}");
    assert!(near(second.left(), first.right() + px(1.)), "{second:?}");
    let origin = first.left();
    let border = |cx: &mut VisualTestContext| {
        let first = cx.debug_bounds("first").unwrap();
        point(first.right() + px(0.5), first.center().y)
    };

    let from = border(cx);
    drag(from, point(origin + px(200.), from.y), cx);
    assert!((ratio(&split, cx) - 200. / 601.).abs() < 0.01);
    assert!(near(cx.debug_bounds("first").unwrap().size.width, px(200.)));

    let from = border(cx);
    drag(from, point(origin + px(10.), from.y), cx);
    assert!(
        (ratio(&split, cx) - 100. / 601.).abs() < 0.01,
        "first kept to 100 px"
    );

    let from = border(cx);
    drag(from, point(origin + px(590.), from.y), cx);
    assert!(
        (ratio(&split, cx) - 451. / 601.).abs() < 0.01,
        "second kept to 150 px"
    );
    let resized = split.read_with(cx, |split, _| split.resized.clone());
    assert!(!resized.is_empty());
    assert_eq!(resized.last().copied(), Some(ratio(&split, cx)));

    // Away from the border, a drag moves nothing.
    let before = ratio(&split, cx);
    let inside = cx.debug_bounds("first").unwrap().center();
    drag(inside, point(inside.x + px(50.), inside.y), cx);
    assert_eq!(ratio(&split, cx), before);
}

#[gpui::test]
fn a_vertical_split_drags_up_and_down(cx: &mut TestAppContext) {
    let (split, cx) = split(Axis::Vertical, cx);
    let first = cx.debug_bounds("first").unwrap();
    assert!(near(first.size.height, px(300.)), "{first:?}");
    let from = point(first.center().x, first.bottom() + px(0.5));
    drag(from, point(from.x, first.top() + px(400.)), cx);
    assert!((ratio(&split, cx) - 400. / 601.).abs() < 0.01);
    let second = cx.debug_bounds("second").unwrap();
    assert!(near(second.size.height, px(200.)), "{second:?}");
}

#[gpui::test]
fn flex_shorthands_lay_out_rows_and_columns(cx: &mut TestAppContext) {
    struct Flex;
    impl Render for Flex {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let cell = |name: &'static str| div().size(px(10.)).debug_selector(move || name.into());
            v_flex()
                .child(h_flex().child(cell("a")).child(cell("b")))
                .child(cell("c"))
        }
    }
    let (_, cx) = cx.add_window_view(|_, _| Flex);
    draw(cx);
    let [a, b, c] = ["a", "b", "c"].map(|name| cx.debug_bounds(name).unwrap());
    assert_eq!((b.left(), b.top()), (a.right(), a.top()), "b beside a");
    assert_eq!((c.left(), c.top()), (a.left(), a.bottom()), "c below them");
}
