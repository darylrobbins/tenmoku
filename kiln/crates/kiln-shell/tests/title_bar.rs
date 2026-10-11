// SPDX-License-Identifier: LicenseRef-Proprietary

//! The unified title bar: the title centred clear of the traffic lights,
//! the content below it, and the window dragged and zoomed from it.

mod support;

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, Pixels, Point, TestAppContext, VisualTestContext,
    point, px,
};
use kiln_platform::window_options::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET};
use kiln_shell::window_frame::WindowRequest;

use support::{linux, mac, open, requests};

fn mouse_down(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

#[gpui::test]
fn only_where_the_title_bar_is_unified(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, linux());
    assert_eq!(cx.debug_bounds("title-bar"), None);
    assert_eq!(cx.debug_bounds("title-bar-label"), None);
}

#[gpui::test]
fn shows_the_title_above_the_content(cx: &mut TestAppContext) {
    let (root, _, cx) = open(cx, mac());
    let size = cx.update(|window, _| window.viewport_size());
    let bar = cx.debug_bounds("title-bar").unwrap();
    assert_eq!(bar.origin, point(px(0.), px(0.)));
    assert_eq!(bar.size.width, size.width);
    assert_eq!(bar.size.height, TITLE_BAR_HEIGHT);
    let label = cx.debug_bounds("title-bar-label").unwrap();
    assert!(near(label.center().x, size.width / 2.));
    let pad = cx.debug_bounds("pad").unwrap();
    assert_eq!(pad.origin.y, TITLE_BAR_HEIGHT);
    assert_eq!(pad.bottom(), size.height);

    // A long title stays out of the traffic lights, in a narrow window.
    cx.simulate_resize(gpui::size(px(300.), px(400.)));
    root.update(cx, |root, cx| root.set_title("x".repeat(100), cx));
    cx.run_until_parked();
    let label = cx.debug_bounds("title-bar-label").unwrap();
    assert!(label.left() >= TRAFFIC_LIGHT_INSET - px(1.), "{label:?}");
    assert!(
        label.right() <= px(300.) - TRAFFIC_LIGHT_INSET + px(1.),
        "{label:?}"
    );
}

#[gpui::test]
fn drag_and_double_click(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, mac());
    let asked = requests(cx);
    let bar = cx.debug_bounds("title-bar").unwrap();
    let label = cx.debug_bounds("title-bar-label").unwrap();
    let empty = point(px(100.), bar.center().y);
    assert!(!label.contains(&empty));

    mouse_down(empty, 1, cx);
    assert_eq!(asked.take(), [WindowRequest::Move]);
    mouse_down(label.center(), 1, cx);
    assert_eq!(asked.take(), [WindowRequest::Move]);
    mouse_down(empty, 2, cx);
    assert_eq!(asked.take(), [WindowRequest::TitlebarDoubleClick]);

    let pad = cx.debug_bounds("pad").unwrap();
    mouse_down(pad.center(), 1, cx);
    mouse_down(pad.center(), 2, cx);
    assert_eq!(asked.take(), []);
}
