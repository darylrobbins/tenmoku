//! [`DiffView`] navigation across a switch from inline to side by side: the
//! left caret inline never placed isn't counted from.

use fe_buffer::Buffer;
use fe_command::Invocation;
use gpui::{Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, size};

use super::*;
use crate::init;

/// Three hunks, rows 1, 4 and 7, the first below row 0 on both sides.
const LEFT: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni";
const RIGHT: &str = "a\nB\nc\nd\nE\nf\ng\nH\ni";

fn diff_view(cx: &mut TestAppContext) -> (Entity<DiffView>, &mut VisualTestContext) {
    cx.update(init);
    let left = EditorModel::new(Buffer::new(LEFT), None);
    let right = EditorModel::new(Buffer::new(RIGHT), Some("a.rs".into()));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(1200.), px(600.)),
        ))),
        ..Default::default()
    };
    let window = cx.update(|cx| {
        cx.open_window(options, move |_, cx| {
            cx.new(|cx| {
                let left = DiffSide::new("Index", left).read_only();
                DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
            })
        })
        .unwrap()
    });
    let diff = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    draw(cx);
    (diff, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn side(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Entity<EditorView> {
    diff.read_with(cx, |diff, _| diff.views[ix].clone())
}

fn toggle(cx: &mut VisualTestContext) {
    let invocation = Invocation::of(TOGGLE_INLINE).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

fn go(diff: &Entity<DiffView>, forward: bool, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        diff.update(cx, |diff, cx| match forward {
            true => diff.next_change(window, cx),
            false => diff.previous_change(window, cx),
        })
    });
    draw(cx);
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| caret_row(view.model()))
}

fn message(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> Option<SharedString> {
    diff.read_with(cx, |diff, _| diff.message.clone())
}

/// Inline to the second hunk and back, the untouched left editor's caret at
/// offset 0 is a caret navigation never placed.
fn inline_then_left_focused(
    app: &mut TestAppContext,
) -> (Entity<DiffView>, Entity<EditorView>, &mut VisualTestContext) {
    let (diff, cx) = diff_view(app);
    toggle(cx);
    go(&diff, true, cx);
    go(&diff, true, cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert_eq!(caret(&right, cx), 4);
    toggle(cx);
    assert!(!diff.read_with(cx, |diff, _| diff.is_inline()));
    cx.update(|window, cx| window.focus(&left.focus_handle(cx), cx));
    assert_eq!(caret(&left, cx), 0);
    (diff, left, cx)
}

#[gpui::test]
fn left_next_change_after_inline_counts_from_its_own_caret(app: &mut TestAppContext) {
    let (diff, left, cx) = inline_then_left_focused(app);
    go(&diff, true, cx);
    assert_eq!(caret(&left, cx), 1);
    assert_eq!(message(&diff, cx), None);
}

#[gpui::test]
fn left_previous_change_after_inline_counts_from_its_own_caret(app: &mut TestAppContext) {
    let (diff, left, cx) = inline_then_left_focused(app);
    go(&diff, false, cx);
    assert_eq!(caret(&left, cx), 7);
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_LAST));
}

#[gpui::test]
fn right_navigation_survives_toggle(app: &mut TestAppContext) {
    let (diff, cx) = diff_view(app);
    let right = side(&diff, 1, cx);
    go(&diff, true, cx);
    go(&diff, true, cx);
    assert_eq!(caret(&right, cx), 4);
    toggle(cx);
    go(&diff, true, cx);
    assert_eq!(caret(&right, cx), 7);
}
