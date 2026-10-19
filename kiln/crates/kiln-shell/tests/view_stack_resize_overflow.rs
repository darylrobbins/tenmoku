// SPDX-License-Identifier: LicenseRef-Proprietary

//! A divider drag over huge weights: when the requested split can't fit an
//! f32 weight, the stack's weights are scaled down alike, so the drag still
//! reaches the minimum body height and the other views keep their share.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, ParentElement as _,
    Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point, px,
};
use kiln_shell::{StackedView, ViewId, ViewStack, view_stack::MIN_VIEW_BODY_HEIGHT};

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
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < px(1.)
}

/// Three expanded views weighted `neighbours`, `neighbours` and `third`.
fn weighted(cx: &mut TestAppContext, neighbours: f32, third: f32) -> Opened<'_> {
    let mut s = open(cx);
    s.stack.update(s.cx, |stack, cx| {
        stack.set_weight(FOLDERS, neighbours, cx);
        stack.set_weight(OUTLINE, neighbours, cx);
        stack.set_weight(TIMELINE, third, cx);
    });
    s.draw();
    s
}

fn drag_huge(cx: &mut TestAppContext, neighbours: f32) {
    let mut s = weighted(cx, neighbours, neighbours);
    let (a, b, c) = (
        s.body_height(FOLDERS).unwrap(),
        s.body_height(OUTLINE).unwrap(),
        s.body_height(TIMELINE).unwrap(),
    );
    let at = s.border(OUTLINE);
    let none = Modifiers::none();
    s.cx.simulate_mouse_down(at, MouseButton::Left, none);
    let step = |s: &mut Opened<'_>, by: Pixels| {
        s.cx.simulate_mouse_move(point(at.x, at.y + by), MouseButton::Left, none);
        s.draw();
        assert!(near(s.body_height(TIMELINE).unwrap(), c), "{by:?}");
        (
            s.body_height(FOLDERS).unwrap(),
            s.body_height(OUTLINE).unwrap(),
        )
    };
    // The first move starts the drag.
    step(&mut s, px(3.));
    // Each neighbour gets the height the pointer asked for.
    let (above, below) = step(&mut s, px(60.));
    assert!(
        near(above, a + px(60.)) && near(below, b - px(60.)),
        "{above:?} {below:?}"
    );
    // Past the end, the divider stops at the minimum body height.
    let (above, below) = step(&mut s, a + b);
    assert!(near(below, MIN_VIEW_BODY_HEIGHT) && near(above, a + b - MIN_VIEW_BODY_HEIGHT));
    // Dragging back moves it across rendered frames.
    let (above, below) = step(&mut s, px(60.));
    assert!(
        near(above, a + px(60.)) && near(below, b - px(60.)),
        "{above:?} {below:?}"
    );
    let (above, below) = step(&mut s, px(-30.));
    assert!(
        near(above, a - px(30.)) && near(below, b + px(30.)),
        "{above:?} {below:?}"
    );
    for view in [FOLDERS, OUTLINE, TIMELINE] {
        let weight = s.weight(view);
        assert!(weight.is_finite() && weight > 0., "{weight}");
    }
    s.cx.simulate_mouse_up(at, MouseButton::Left, none);
}

#[gpui::test]
fn a_drag_over_huge_weights_reaches_the_minimum(cx: &mut TestAppContext) {
    drag_huge(cx, 2e38);
}

#[gpui::test]
fn a_drag_over_f32_max_weights_reaches_the_minimum(cx: &mut TestAppContext) {
    drag_huge(cx, f32::MAX);
}

#[gpui::test]
fn a_smaller_third_view_keeps_its_share(cx: &mut TestAppContext) {
    let mut s = weighted(cx, f32::MAX, 1e30);
    let (a, b, c) = (
        s.body_height(FOLDERS).unwrap(),
        s.body_height(OUTLINE).unwrap(),
        s.body_height(TIMELINE).unwrap(),
    );
    s.drag(OUTLINE, a + b);
    assert!(near(s.body_height(OUTLINE).unwrap(), MIN_VIEW_BODY_HEIGHT));
    assert!(near(
        s.body_height(FOLDERS).unwrap(),
        a + b - MIN_VIEW_BODY_HEIGHT
    ));
    assert!(near(s.body_height(TIMELINE).unwrap(), c));
}

#[gpui::test]
fn scaling_keeps_a_subnormal_weights_share(cx: &mut TestAppContext) {
    let mut s = weighted(cx, f32::MAX, 1e-40);
    s.drag(OUTLINE, px(100.));
    let pair = f64::from(s.weight(FOLDERS)) + f64::from(s.weight(OUTLINE));
    let ratio = f64::from(s.weight(TIMELINE)) / pair;
    let expected = 1e-40 / (2. * f64::from(f32::MAX));
    assert!(s.weight(FOLDERS) < f32::MAX || s.weight(OUTLINE) < f32::MAX);
    assert!(
        (ratio / expected - 1.).abs() < 0.05,
        "{ratio:e} {expected:e}"
    );
}
