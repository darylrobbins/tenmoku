// SPDX-License-Identifier: Apache-2.0

//! A view stack's dividers: dragging one moves weight between the two
//! views either side down to a minimum body height, a double-click gives
//! the pair equal weights, a collapsed view has none either side, and the
//! weights survive save and restore.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, ParentElement as _, Pixels, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, point, px,
};
use kiln_shell::{
    StackedView, ViewId, ViewStack,
    view_stack::{MIN_VIEW_BODY_HEIGHT, VIEW_HEADER_HEIGHT},
};

const FOLDERS: ViewId = ViewId("folders");
const OUTLINE: ViewId = ViewId("outline");
const TIMELINE: ViewId = ViewId("timeline");

struct Body;

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

struct Host(Entity<ViewStack>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

struct Opened<'a> {
    stack: Entity<ViewStack>,
    cx: &'a mut VisualTestContext,
}

/// Three expanded views of weight 1 in a drawn window.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let (host, cx) = cx.add_window_view(|_, cx| {
        let stack = cx.new(|cx| {
            let mut stack = ViewStack::new();
            for (id, title) in [
                (FOLDERS, "Folders"),
                (OUTLINE, "Outline"),
                (TIMELINE, "Timeline"),
            ] {
                stack.add(StackedView::new(id, title, cx.new(|_| Body)), cx);
            }
            stack
        });
        Host(stack)
    });
    let stack = host.read_with(cx, |host, _| host.0.clone());
    let mut opened = Opened { stack, cx };
    opened.draw();
    opened
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn weight(&mut self, view: ViewId) -> f32 {
        self.stack
            .read_with(self.cx, |stack, _| stack.weight(view))
            .expect("in the stack")
    }

    fn body_height(&mut self, view: ViewId) -> Option<Pixels> {
        self.stack
            .read_with(self.cx, |stack, _| stack.body_bounds(view))
            .map(|body| body.size.height)
    }

    /// Where the border above `view` is: the top of its header.
    fn border(&mut self, view: ViewId) -> gpui::Point<Pixels> {
        let header = self
            .stack
            .read_with(self.cx, |stack, _| stack.header_bounds(view))
            .expect("drawn");
        point(header.center().x, header.top() + px(1.))
    }

    /// Presses on the border above `view`, drags it `by` down, releases.
    fn drag(&mut self, view: ViewId, by: Pixels) {
        let at = self.border(view);
        let to = point(at.x, at.y + by);
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(at, MouseButton::Left, none);
        self.cx
            .simulate_mouse_move(point(at.x, at.y + px(3.)), MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_up(to, MouseButton::Left, none);
        self.draw();
    }

    fn double_click(&mut self, view: ViewId) {
        let at = self.border(view);
        for click_count in [1, 2] {
            let (position, modifiers) = (at, Modifiers::none());
            self.cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count,
            });
        }
        self.draw();
    }

    fn collapse(&mut self, view: ViewId) {
        self.stack
            .update(self.cx, |stack, cx| stack.set_collapsed(view, true, cx));
        self.draw();
    }
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < px(1.)
}

#[gpui::test]
fn a_drag_moves_weight_between_the_neighbours(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (a, b, c) = (
        s.body_height(FOLDERS).unwrap(),
        s.body_height(OUTLINE).unwrap(),
        s.body_height(TIMELINE).unwrap(),
    );
    s.drag(OUTLINE, px(20.));
    assert!(near(s.body_height(FOLDERS).unwrap(), a + px(20.)));
    assert!(near(s.body_height(OUTLINE).unwrap(), b - px(20.)));
    assert!(near(s.body_height(TIMELINE).unwrap(), c));
    let total = s.weight(FOLDERS) + s.weight(OUTLINE);
    assert!((total - 2.).abs() < 1e-4, "{total}");
    assert_eq!(s.weight(TIMELINE), 1.);
}

#[gpui::test]
fn a_drag_stops_at_the_minimum_body_height(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let pair = s.body_height(FOLDERS).unwrap() + s.body_height(OUTLINE).unwrap();
    s.drag(OUTLINE, px(10000.));
    assert!(near(s.body_height(OUTLINE).unwrap(), MIN_VIEW_BODY_HEIGHT));
    assert!(near(
        s.body_height(FOLDERS).unwrap(),
        pair - MIN_VIEW_BODY_HEIGHT
    ));
    s.drag(OUTLINE, px(-10000.));
    assert!(near(s.body_height(FOLDERS).unwrap(), MIN_VIEW_BODY_HEIGHT));
    assert!(near(
        s.body_height(OUTLINE).unwrap(),
        pair - MIN_VIEW_BODY_HEIGHT
    ));
}

#[gpui::test]
fn a_double_click_makes_the_pair_equal(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.drag(OUTLINE, px(30.));
    assert_ne!(s.weight(FOLDERS), s.weight(OUTLINE));
    s.double_click(OUTLINE);
    assert_eq!(s.weight(FOLDERS), s.weight(OUTLINE));
    assert!((s.weight(FOLDERS) - 1.).abs() < 1e-4);
    assert_eq!(s.weight(TIMELINE), 1.);
}

#[gpui::test]
fn a_collapsed_view_has_no_divider(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.collapse(OUTLINE);
    // Neither border of the collapsed view is a divider.
    s.drag(OUTLINE, px(20.));
    s.drag(TIMELINE, px(20.));
    s.double_click(TIMELINE);
    for view in [FOLDERS, OUTLINE, TIMELINE] {
        assert_eq!(s.weight(view), 1., "{view:?}");
    }
    assert!(near(
        s.border(OUTLINE).y,
        s.border(FOLDERS).y + VIEW_HEADER_HEIGHT + s.body_height(FOLDERS).unwrap()
    ));
}

#[gpui::test]
fn resized_weights_survive_save_and_restore(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.drag(TIMELINE, px(-25.));
    let weights = [FOLDERS, OUTLINE, TIMELINE].map(|view| s.weight(view));
    let saved = s.stack.read_with(s.cx, |stack, _| stack.save());
    s.double_click(TIMELINE);
    s.double_click(OUTLINE);
    s.stack.update(s.cx, |stack, cx| stack.restore(&saved, cx));
    assert_eq!(
        [FOLDERS, OUTLINE, TIMELINE].map(|view| s.weight(view)),
        weights
    );
}

#[gpui::test]
fn huge_weights_stay_finite_on_drag_and_reset(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.stack.update(s.cx, |stack, cx| {
        stack.set_weight(FOLDERS, 2e38, cx);
        stack.set_weight(OUTLINE, 2e38, cx);
    });
    s.draw();
    s.drag(OUTLINE, px(20.));
    for view in [FOLDERS, OUTLINE] {
        let weight = s.weight(view);
        assert!(weight.is_finite() && weight > 0., "{weight}");
    }
    s.double_click(OUTLINE);
    for view in [FOLDERS, OUTLINE] {
        let weight = s.weight(view);
        assert!(weight.is_finite() && weight > 0., "{weight}");
    }
}

#[gpui::test]
fn a_change_from_elsewhere_ends_the_drag(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let at = s.border(OUTLINE);
    let none = Modifiers::none();
    s.cx.simulate_mouse_down(at, MouseButton::Left, none);
    s.cx.simulate_mouse_move(point(at.x, at.y + px(10.)), MouseButton::Left, none);
    s.draw();
    s.stack
        .update(s.cx, |stack, cx| stack.set_weight(OUTLINE, 5., cx));
    s.draw();
    s.cx.simulate_mouse_move(point(at.x, at.y + px(30.)), MouseButton::Left, none);
    s.draw();
    assert_eq!(s.weight(OUTLINE), 5.);
    s.cx.simulate_mouse_up(at, MouseButton::Left, none);
}

/// Two stacks side by side, with the same view ids.
struct Pair(Entity<ViewStack>, Entity<ViewStack>);

impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(div().flex_1().child(self.0.clone()))
            .child(div().flex_1().child(self.1.clone()))
    }
}

#[gpui::test]
fn a_drag_in_one_stack_leaves_another_alone(cx: &mut TestAppContext) {
    let (pair, cx) = cx.add_window_view(|_, cx| {
        let make = |cx: &mut Context<Pair>| {
            cx.new(|cx| {
                let mut stack = ViewStack::new();
                for (id, title) in [(FOLDERS, "Folders"), (OUTLINE, "Outline")] {
                    stack.add(StackedView::new(id, title, cx.new(|_| Body)), cx);
                }
                stack
            })
        };
        Pair(make(cx), make(cx))
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (left, right) = pair.read_with(cx, |pair, _| (pair.0.clone(), pair.1.clone()));
    let header = left
        .read_with(cx, |stack, _| stack.header_bounds(OUTLINE))
        .unwrap();
    let at = point(header.center().x, header.top() + px(1.));
    let none = Modifiers::none();
    cx.simulate_mouse_down(at, MouseButton::Left, none);
    cx.simulate_mouse_move(point(at.x, at.y + px(3.)), MouseButton::Left, none);
    cx.simulate_mouse_move(point(at.x, at.y + px(30.)), MouseButton::Left, none);
    cx.run_until_parked();
    let weights = |stack: &Entity<ViewStack>, cx: &mut VisualTestContext| {
        stack.read_with(cx, |stack, _| {
            (
                stack.weight(FOLDERS).unwrap(),
                stack.weight(OUTLINE).unwrap(),
            )
        })
    };
    assert_ne!(weights(&left, cx).0, 1.);
    assert_eq!(weights(&right, cx), (1., 1.));
    cx.simulate_mouse_up(at, MouseButton::Left, none);
}

#[gpui::test]
fn huge_weights_keep_the_third_view_and_the_drag_going(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.stack.update(s.cx, |stack, cx| {
        for view in [FOLDERS, OUTLINE, TIMELINE] {
            stack.set_weight(view, 2e38, cx);
        }
    });
    s.draw();
    let third = s.body_height(TIMELINE).unwrap();
    let at = s.border(OUTLINE);
    let none = Modifiers::none();
    s.cx.simulate_mouse_down(at, MouseButton::Left, none);
    for by in [3., 60., 120., 20.] {
        s.cx.simulate_mouse_move(point(at.x, at.y + px(by)), MouseButton::Left, none);
        s.draw();
        assert!(near(s.body_height(TIMELINE).unwrap(), third), "{by}");
        for view in [FOLDERS, OUTLINE, TIMELINE] {
            let weight = s.weight(view);
            assert!(weight.is_finite() && weight > 0., "{weight}");
        }
    }
    // Back near the start, so the drag is still live.
    let moved = s.body_height(FOLDERS).unwrap() - third;
    assert!(moved < px(40.), "{moved:?}");
    s.cx.simulate_mouse_up(at, MouseButton::Left, none);
}
