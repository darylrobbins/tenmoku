//! Search highlighting has one time limit per painted frame, and a frame
//! that ran out is not searched again until the pattern or the text changes.

use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext};

use super::{FOUND, TEST_LIMIT};
use crate::{EditorTheme, EditorView, commands::RunCommand, init, set_vim_enabled};

fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn submit(key: &str, text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(key);
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": text }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| view.painted_matches.clone())
}

/// Forgets that an earlier frame (the keys that set up the search drew some)
/// ran out, so the next draw is the first to search.
fn unlatch(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().highlight_ran_out = None;
    });
}

fn found() -> usize {
    FOUND.with(|n| n.get())
}

#[gpui::test]
fn one_limit_per_frame(cx: &mut TestAppContext) {
    TEST_LIMIT.with(|l| l.set(Some(Duration::from_millis(100))));
    let (view, cx) = editor(cx, &format!("{}\n", "a".repeat(200_000)).repeat(20));
    submit("/", r"a.*z\|a", cx);
    unlatch(&view, cx);
    let started = Instant::now();
    let painted = draw(&view, cx);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert!(!painted.is_empty());
    assert!(painted.iter().all(|m| m.origin.y == painted[0].origin.y));
    let first_row_y = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.frame.row_top(0)
    });
    let top = painted[0].origin.y;
    let origin = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().origin.y);
    assert!((f32::from(top - origin) - first_row_y).abs() < 0.01);
    TEST_LIMIT.with(|l| l.set(None));
}

#[gpui::test]
fn ran_out_latches_until_a_change(cx: &mut TestAppContext) {
    // A zero limit has run out by the frame's first clock read, whatever the
    // machine's load; later frames get all the time they need, so that only
    // the latch keeps them from searching.
    TEST_LIMIT.with(|l| l.set(Some(Duration::ZERO)));
    let (view, cx) = editor(cx, &format!("{}\n", "a".repeat(1_000)).repeat(20));
    submit("/", r"a.*z\|a", cx);
    unlatch(&view, cx);
    assert!(draw(&view, cx).is_empty());
    TEST_LIMIT.with(|l| l.set(Some(Duration::from_secs(3600))));

    assert!(draw(&view, cx).is_empty());
    assert_eq!(found(), 0);

    view.update(cx, |view, _| view.model_mut().insert("b"));
    assert!(!draw(&view, cx).is_empty());
    assert!(found() > 0);

    TEST_LIMIT.with(|l| l.set(Some(Duration::ZERO)));
    submit("/", "a", cx);
    unlatch(&view, cx);
    assert!(draw(&view, cx).is_empty());
    TEST_LIMIT.with(|l| l.set(Some(Duration::from_secs(3600))));
    submit("/", "b", cx);
    assert!(!draw(&view, cx).is_empty());
    TEST_LIMIT.with(|l| l.set(None));
}
