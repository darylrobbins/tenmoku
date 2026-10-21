//! Vim's search matches as painted in the editor: shown under the
//! selection, capped per frame, cleared by `:noh`.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px, size};

use super::*;
use crate::{EditorTheme, EditorView, commands::RunCommand, init};

fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    vim: bool,
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
    cx.update(|_, cx| set_vim_enabled(cx, vim));
    cx.run_until_parked();
    (view, cx)
}

/// Opens the line with `key` and submits `text` on it.
fn submit(key: &str, text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(key);
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": text }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

fn matches(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| view.painted_matches.clone())
}

/// The window bounds of the match at `range`, as the bar carets at its ends
/// put them.
fn expected(
    view: &Entity<EditorView>,
    range: std::ops::Range<usize>,
    cx: &mut VisualTestContext,
) -> (Pixels, Pixels, Pixels) {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let (left, right) = (
            layout.caret_bounds(range.start).unwrap(),
            layout.caret_bounds(range.end).unwrap(),
        );
        (left.origin.x, right.origin.x - left.origin.x, left.origin.y)
    })
}

#[gpui::test]
fn matches_are_highlighted(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab ab\nxab", true);
    assert!(matches(&view, cx).is_empty());

    submit("/", "ab", cx);
    let shown = matches(&view, cx);
    assert_eq!(shown.len(), 3);
    for (found, range) in shown.iter().zip([0..2, 3..5, 7..9]) {
        let (x, width, y) = expected(&view, range, cx);
        assert_eq!(
            (found.origin.x, found.size.width, found.origin.y),
            (x, width, y)
        );
    }

    // :noh clears them; n shows them again.
    submit(":", "noh", cx);
    assert!(matches(&view, cx).is_empty());
    cx.simulate_keystrokes("n");
    assert_eq!(matches(&view, cx).len(), 3);

    // Deleting a match removes its rectangle on the next frame.
    cx.simulate_keystrokes("g g 0 x");
    assert_eq!(
        view.read_with(cx, |view, _| view.model().text().to_string()),
        "b ab\nxab"
    );
    assert_eq!(matches(&view, cx).len(), 2);
}

fn wrap(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A frame's buffer rows, first to last, as drawn.
fn shown_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> std::ops::Range<u32> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        let rows = &view.layout.as_ref().unwrap().frame.rows;
        rows.first().unwrap().buffer_row..rows.last().unwrap().buffer_row + 1
    })
}

/// The matches on the rows in view, in order, that reach the editor's text
/// area, with the window bounds of a bar at their start.
fn visible_ranges(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(std::ops::Range<usize>, Bounds<Pixels>)> {
    let rows = shown_rows(view, cx);
    view.update(cx, |view, _| {
        let pattern = view.vim.as_ref().unwrap().highlight().unwrap().clone();
        let all = fe_vim::search::matches_in(view.model().text(), rows, &pattern, usize::MAX);
        let layout = view.layout.as_mut().unwrap();
        let text = layout.bounds(layout.frame.text_bounds());
        all.into_iter()
            .filter_map(|range| {
                let start = layout.caret_bounds(range.start)?;
                let end = layout.caret_bounds(range.end)?;
                let reaches = start.origin.x < text.right() && end.origin.x > text.left();
                let bounds = Bounds::new(
                    start.origin,
                    size(end.origin.x - start.origin.x, start.size.height),
                );
                reaches.then_some((range, bounds))
            })
            .collect()
    })
}

/// In a tiny font the window holds far more than the cap's worth of matches.
#[gpui::test]
fn matches_are_capped_per_frame(cx: &mut TestAppContext) {
    let line = "a ".repeat(100);
    let (view, cx) = editor(cx, &vec![line; 2_000].join("\n"), true);
    view.update(cx, |view, _| view.theme.font_size = px(3.));
    submit("/", "a", cx);
    let visible = visible_ranges(&view, cx);
    assert!(visible.len() >= 10_000, "{}", visible.len());
    let painted = matches(&view, cx);
    assert_eq!(painted.len(), crate::element::MAX_MATCHES);
    // Exactly the first matches in view, in order.
    for ((range, bar), found) in visible.iter().zip(&painted) {
        assert_eq!(
            (found.origin.x, found.origin.y),
            (bar.origin.x, bar.origin.y),
            "{range:?}"
        );
    }
}

/// Matches scrolled off the left of a huge line, more than ten caps of
/// them, don't use the budget.
#[gpui::test]
fn offscreen_matches_dont_starve_visible_ones(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, &"x ".repeat(6_000), true);
    submit("/", "x", cx);
    view.update(cx, |view, _| {
        view.scroll.x = px(150_000.);
        view.autoscroll = false;
    });
    let shown = matches(&view, cx);
    assert!(!shown.is_empty());
    assert!(shown.len() < crate::element::MAX_MATCHES);
}

/// A zero-width match at a soft wrap point is on the row after it, which
/// isn't shown, not at the end of the bottom row.
#[gpui::test]
fn zero_width_match_at_a_wrap_point_is_not_on_the_row_before(cx: &mut TestAppContext) {
    let word = "a".repeat(20_000);
    let (view, cx) = editor(cx, &word, true);
    wrap(&view, cx);
    let end = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.rows.last().unwrap().end
    });
    let (view, cx) = editor(cx, &format!("{} {word}", "a".repeat(end - 1)), true);
    wrap(&view, cx);
    submit("/", "\\<", cx);
    // Only the word start at the text's start, none at the wrap point.
    assert_eq!(matches(&view, cx).len(), 1);
}

/// A match right after the first character of a long row's displayed slice,
/// a character with a combining mark, is painted as `matches_in` finds it.
/// The match is long enough to reach the text area from the shaped slice's
/// start, which lies some way off to its left.
#[gpui::test]
fn long_row_matches_follow_enumeration_after_a_grapheme(cx: &mut TestAppContext) {
    // "a\u{301}b " is 5 bytes; shift the text until the slice starts on an "a".
    for pad in 0..5 {
        let text = format!("{}{}", "x".repeat(pad), "a\u{301}b ".repeat(6_000));
        let (view, cx) = editor(cx, &text, true);
        submit("/", "\u{301}b.\\{200}", cx);
        view.update(cx, |view, _| {
            view.scroll.x = px(25_000.);
            view.autoscroll = false;
        });
        let expected = visible_ranges(&view, cx);
        let start = view.read_with(cx, |view, _| {
            view.layout.as_ref().unwrap().frame.rows[0].shown().start
        });
        if (start - pad) % 5 != 0 {
            continue;
        }
        // The match spanning the slice's first grapheme, which reaches the
        // text area, is among them; one found from the slice's start would
        // begin after it.
        assert!(
            expected
                .iter()
                .any(|(range, _)| range.start <= start && range.end > start + 1),
            "{start}: {:?}",
            expected.iter().map(|(range, _)| range).collect::<Vec<_>>()
        );
        let painted = matches(&view, cx);
        assert_eq!(painted.len(), expected.len());
        for ((_, bar), found) in expected.iter().zip(&painted) {
            for (got, want) in [
                (found.origin.x, bar.origin.x),
                (found.origin.y, bar.origin.y),
                (found.size.width, bar.size.width),
                (found.size.height, bar.size.height),
            ] {
                assert!((got - want).abs() < px(0.01), "{found:?} vs {bar:?}");
            }
        }
        return;
    }
    panic!("no padding put the slice start on an \"a\"");
}

/// A huge line's matches are enumerated as far as what's shown, not to the
/// end of the line, with vim's search as with find's.
#[gpui::test]
fn enumeration_stops_after_what_is_shown(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, &"x ".repeat(200_000), true);
    submit("/", "x", cx);
    let painted = matches(&view, cx).len();
    assert!(painted > 0);
    let found = crate::element::FOUND.with(|n| n.get());
    assert!(found < 10_000, "{found} enumerated for {painted} painted");
}

/// Scrolled down a soft-wrapped huge line, the matches looked at are the
/// shown ones, not all the line's.
#[gpui::test]
fn work_is_bounded_by_what_is_shown(cx: &mut TestAppContext) {
    // No time limit to run out of on a loaded machine.
    crate::element::TEST_LIMIT.with(|l| l.set(Some(std::time::Duration::from_secs(3600))));
    let (view, cx) = editor(cx, &"x ".repeat(100_000), true);
    wrap(&view, cx);
    submit("/", "x", cx);
    view.update(cx, |view, _| {
        view.scroll.y = px(20_000.);
        view.autoscroll = false;
    });
    let painted = matches(&view, cx).len();
    let rows = shown_rows(&view, cx);
    assert_eq!(rows, 0..1);
    let looked_at = crate::element::CANDIDATES.with(|n| n.get());
    assert!(painted > 0);
    assert!(looked_at <= painted + 64, "{looked_at} for {painted}");
    crate::element::TEST_LIMIT.with(|l| l.set(None));
}

/// `^` and `$` match nothing wide: each shows as a cell, empty rows too.
#[gpui::test]
fn zero_width_matches_show_as_a_cell(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab\n\ncd", true);
    submit("/", "^", cx);
    assert_eq!(matches(&view, cx).len(), 3);
    submit("/", "$", cx);
    assert_eq!(matches(&view, cx).len(), 3);
}

#[gpui::test]
fn nothing_with_vim_off(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab ab", false);
    assert!(matches(&view, cx).is_empty());
}

#[path = "multiline_highlight_tests.rs"]
mod multiline_highlight_tests;
