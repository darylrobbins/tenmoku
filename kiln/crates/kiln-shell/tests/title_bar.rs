// SPDX-License-Identifier: Apache-2.0

//! The unified title bar: 38 px with the traffic lights at (14, 12), the
//! title centred clear of them or the app's items after them and at the
//! far end, the content below it, and the window dragged and zoomed from
//! its empty space.

use std::{cell::Cell, rc::Rc};

use crate::support;

use gpui::{
    Context, InteractiveElement as _, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, VisualTestContext, Window, div, point, px,
};
use kiln_platform::window_options::{
    TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options,
};
use kiln_shell::{TitleBar, TitleBarItems, TitleBarSlot, window_frame::WindowRequest};

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

#[gpui::test]
fn the_bar_is_38_px_with_the_lights_at_14_12(cx: &mut TestAppContext) {
    assert_eq!(TITLE_BAR_HEIGHT, px(38.));
    assert_eq!(TRAFFIC_LIGHTS, point(px(14.), px(12.)));
    // 14 px buttons, centred in the bar.
    assert_eq!(TRAFFIC_LIGHTS.y * 2. + px(14.), TITLE_BAR_HEIGHT);
    // Three buttons 6 px apart from 14 px in, then a 12 px gap.
    assert_eq!(TRAFFIC_LIGHT_INSET, px(14. + 3. * 14. + 2. * 6. + 12.));
    let options = titlebar_options("Notes");
    assert_eq!(options.traffic_light_position, Some(TRAFFIC_LIGHTS));
    assert!(options.appears_transparent);

    let (_, _, cx) = open(cx, mac());
    let bar = cx.debug_bounds("title-bar").unwrap();
    assert_eq!(bar.size.height, px(38.));
}

/// A window of a bare [`TitleBar`] with `items`, over a pad.
struct Bar {
    items: TitleBarItems,
}

impl Render for Bar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(TitleBar::new("Notes").items(self.items.clone()))
            .child(div().flex_1().debug_selector(|| "pad".into()))
    }
}

/// A slot of one 40 by 20 item, `name`, whose clicks count in `clicks`.
fn item(name: &'static str, clicks: Rc<Cell<usize>>) -> TitleBarSlot {
    Rc::new(move |_, _| {
        let clicks = clicks.clone();
        div()
            .id(name)
            .debug_selector(move || name.into())
            .w(px(40.))
            .h(px(20.))
            .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
            .into_any_element()
    })
}

#[gpui::test]
fn items_sit_after_the_lights_and_at_the_far_end(cx: &mut TestAppContext) {
    let clicks = Rc::new(Cell::new(0));
    let items = TitleBarItems {
        left: Some(item("left-item", clicks.clone())),
        right: Some(item("right-item", clicks.clone())),
    };
    let (_, cx) = cx.add_window_view(|_, _| Bar { items });
    cx.run_until_parked();
    let asked = requests(cx);
    let width = cx.update(|window, _| window.viewport_size().width);
    let bar = cx.debug_bounds("title-bar").unwrap();
    assert_eq!(bar.size.height, TITLE_BAR_HEIGHT);

    // The left items take the title's place, right after the lights.
    assert_eq!(cx.debug_bounds("title-bar-label"), None);
    let left = cx.debug_bounds("left-item").unwrap();
    assert_eq!(left.left(), TRAFFIC_LIGHT_INSET);
    assert!(near(left.center().y, bar.center().y), "{left:?}");
    let right = cx.debug_bounds("right-item").unwrap();
    assert!(right.right() < width && right.right() >= width - px(16.), "{right:?}");
    assert!(near(right.center().y, bar.center().y), "{right:?}");

    // A press on an item is the item's: it clicks it, and doesn't move the
    // window; the bar between them still does.
    for at in [left.center(), right.center()] {
        cx.simulate_click(at, Modifiers::none());
        mouse_down(at, 2, cx);
    }
    assert_eq!(clicks.get(), 2);
    assert_eq!(asked.take(), []);
    mouse_down(point(width / 2., bar.center().y), 1, cx);
    assert_eq!(asked.take(), [WindowRequest::Move]);
}

#[gpui::test]
fn right_items_alone_keep_the_title_centred(cx: &mut TestAppContext) {
    let clicks = Rc::new(Cell::new(0));
    let items = TitleBarItems {
        left: None,
        right: Some(item("right-item", clicks)),
    };
    let (_, cx) = cx.add_window_view(|_, _| Bar { items });
    cx.run_until_parked();
    let width = cx.update(|window, _| window.viewport_size().width);
    let label = cx.debug_bounds("title-bar-label").unwrap();
    assert!(near(label.center().x, width / 2.));
    let right = cx.debug_bounds("right-item").unwrap();
    assert!(right.right() < width && right.left() > label.right(), "{right:?}");
}
