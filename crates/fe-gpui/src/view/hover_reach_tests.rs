//! Reaching the hover popover: it stays while the pointer travels to it and
//! rests on it, takes its clicks, and scrolls a long text with the wheel.

use std::time::Duration;

use gpui::{
    Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};

use super::{at, calls, editor, move_to, rest, word};
use crate::HoverContent;

fn shown(view: &gpui::Entity<crate::EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.popover_text())
}

fn line_height(view: &gpui::Entity<crate::EditorView>, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        px(view.layout.as_ref().unwrap().frame.line_height)
    })
}

fn wheel(position: Point<Pixels>, lines: f32, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0., lines)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
}

fn advance(millis: u64, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
}

#[gpui::test]
fn the_pointer_reaches_the_popover_and_stays_on_it(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(0, cx);
    // [`super::DOCS`], its fence dropped.
    let docs = Some("let alpha: i32\nThe *alpha* value.".to_owned());
    let alpha = word("alpha");

    rest(at(&view, alpha.start + 1, cx), cx);
    assert_eq!(shown(&view, cx), docs);
    let popover = cx.debug_bounds("hover-popover").expect("drawn");

    // Straight down off the row, onto the popover, then resting on it.
    let on_word = at(&view, alpha.start + 1, cx);
    let row_bottom = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        layout.caret_bounds(alpha.start + 1).unwrap().bottom()
    });
    move_to(point(on_word.x, row_bottom + px(4.)), cx);
    move_to(popover.center(), cx);
    advance(1000, cx);
    assert_eq!(shown(&view, cx), docs);
    assert_eq!(calls(&provider), [alpha.start + 1]);

    // A click in it leaves the selections, and it, be.
    let selections = view.read_with(cx, |view, _| view.model.selections());
    cx.simulate_click(popover.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.model.selections()),
        selections
    );
    assert_eq!(shown(&view, cx), docs);

    // Across other text to it, in time: it stays, and the other word
    // isn't asked about.
    let total = word("total");
    move_to(at(&view, alpha.start + 1, cx), cx);
    move_to(at(&view, total.start + 1, cx), cx);
    advance(100, cx);
    move_to(popover.center(), cx);
    advance(1000, cx);
    assert_eq!(shown(&view, cx), docs);
    assert_eq!(calls(&provider), [alpha.start + 1]);

    // Off both, it goes once the grace is up.
    let viewport = cx.update(|window, _| window.viewport_size());
    let below = point(px(4.), viewport.height - px(4.));
    assert!(!popover.contains(&below));
    move_to(below, cx);
    advance(1000, cx);
    assert_eq!(shown(&view, cx), None);

    // `escape` with the pointer on it hides it at once.
    rest(at(&view, alpha.start + 1, cx), cx);
    assert_eq!(shown(&view, cx), docs);
    let popover = cx.debug_bounds("hover-popover").expect("drawn");
    move_to(popover.center(), cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(shown(&view, cx), None);
}

#[gpui::test]
fn the_wheel_scrolls_a_tall_popover(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(0, cx);
    let long: Vec<String> = (1..=60).map(|line| format!("line {line}")).collect();
    *provider.reply.borrow_mut() = Some(HoverContent {
        text: long.join("\n\n"),
        range: None,
    });
    let alpha = word("alpha");
    rest(at(&view, alpha.start + 1, cx), cx);
    assert!(shown(&view, cx).is_some());

    // At most 16 lines tall, with its padding and border.
    let line = line_height(&view, cx);
    let popover = cx.debug_bounds("hover-popover").expect("drawn");
    assert!(
        popover.size.height <= line * 16. + px(12.),
        "{popover:?} for lines of {line:?}"
    );

    // The wheel over it scrolls it, not the editor, and keeps it.
    let scroll = view.read_with(cx, |view, _| view.scroll);
    wheel(popover.center(), -5., cx);
    assert_eq!(view.read_with(cx, |view, _| view.scroll), scroll);
    assert!(shown(&view, cx).is_some());
    let content = cx.debug_bounds("hover-popover-content").expect("drawn");
    let body = cx.debug_bounds("hover-popover-body").expect("drawn");
    assert!(body.top() < content.top() - line, "{body:?} in {content:?}");

    // Over the text outside it, the wheel scrolls the editor and hides it.
    wheel(at(&view, word("main").start, cx), -1., cx);
    assert_ne!(view.read_with(cx, |view, _| view.scroll), scroll);
    assert_eq!(shown(&view, cx), None);
}
