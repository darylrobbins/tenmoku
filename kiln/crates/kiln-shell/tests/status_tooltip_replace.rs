// SPDX-License-Identifier: Apache-2.0

//! A status item replaced under the same id while its tooltip is pending or
//! shown: the new tooltip text is the one shown.

use std::time::Duration;

use gpui::{
    Context, Entity, IntoElement, Modifiers, ParentElement as _, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};
use kiln_shell::{Icon, StatusItem, StatusSide, StatusTone, status};

/// A view with the bar along its bottom, drawing `items`.
struct Bench {
    items: Vec<StatusItem>,
}

impl Render for Bench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1())
            .child(status::status_bar(self.items.clone(), cx))
    }
}

fn errors(label: &str, tooltip: &str) -> Vec<StatusItem> {
    vec![
        StatusItem::new("errors", StatusSide::Left, 0, label.to_owned())
            .icon(Icon::CircleX)
            .tone(StatusTone::Error)
            .tooltip(tooltip.to_owned()),
        StatusItem::new("plain", StatusSide::Left, 1, "Plain"),
    ]
}

fn shown(text: &str, cx: &mut VisualTestContext) -> bool {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(format!("status-tooltip:errors:{text}").leak())
        .is_some()
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(selector).expect("drawn")
}

fn advance(ms: u64, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}

fn replace(view: &Entity<Bench>, cx: &mut VisualTestContext) {
    view.update(cx, |b, cx| {
        b.items = errors("1", "1 error");
        cx.notify();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn tooltip_text_follows_replacement_while_waiting(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Bench {
        items: errors("2", "2 errors"),
    });
    cx.run_until_parked();
    let item = bounds("status-item:errors", cx);
    cx.simulate_mouse_move(item.center(), None, Modifiers::none());
    advance(200, cx);
    assert!(!shown("2 errors", cx));

    replace(&view, cx);
    advance(600, cx);
    assert!(shown("1 error", cx));
    assert!(!shown("2 errors", cx));
}

#[gpui::test]
fn tooltip_text_follows_replacement_while_shown(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Bench {
        items: errors("2", "2 errors"),
    });
    cx.run_until_parked();
    let item = bounds("status-item:errors", cx);
    cx.simulate_mouse_move(item.center(), None, Modifiers::none());
    advance(600, cx);
    assert!(shown("2 errors", cx));

    replace(&view, cx);
    assert!(shown("1 error", cx));
    assert!(!shown("2 errors", cx));

    let plain = bounds("status-item:plain", cx);
    cx.simulate_mouse_move(plain.center(), None, Modifiers::none());
    advance(600, cx);
    assert!(!shown("1 error", cx));
    assert!(!shown("2 errors", cx));
}
