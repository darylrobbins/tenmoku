//! A vim search whose pattern matches across rows, as painted: each row's
//! part of a match, a match starting above the first row in view too.

#![allow(clippy::single_range_in_vec_init)]

use super::*;

/// The window bounds of each displayed row's part of the matches at
/// `ranges`: from the start of the part to its end on the row (the row's
/// end for a match going on to the next), as bar carets there put them, and
/// past a row's end one space wide for its line break if the match takes
/// that in, as nvim paints it: a blank row's is all there is of it.
fn parts(
    view: &Entity<EditorView>,
    ranges: &[std::ops::Range<usize>],
    cx: &mut VisualTestContext,
) -> Vec<(Pixels, Pixels, Pixels)> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let space = layout.space_width();
        let rows: Vec<_> = (layout.frame.rows.iter())
            .map(|row| (row.start, row.end, row.last))
            .collect();
        let mut parts = Vec::new();
        for (row_start, row_end, last) in rows {
            for range in ranges {
                let (start, end) = (range.start.max(row_start), range.end.min(row_end));
                let line_break = last && range.start <= row_end && range.end > row_end;
                if start >= end && !line_break {
                    continue;
                }
                let (left, right) = (
                    layout.caret_bounds(start).unwrap(),
                    layout.caret_bounds(end).unwrap(),
                );
                let cell = if line_break { space } else { px(0.) };
                parts.push((
                    left.origin.x,
                    right.origin.x - left.origin.x + cell,
                    left.origin.y,
                ));
            }
        }
        parts
    })
}

#[gpui::test]
fn a_match_across_rows_paints_each_part(cx: &mut TestAppContext) {
    // "abc\ndef\n" is 8 bytes: each `c\nd` from 8k+2 to 8k+5.
    let (view, cx) = editor(cx, &vec!["abc\ndef"; 100].join("\n"), true);
    submit("/", "c\\nd", cx);
    let all: Vec<_> = (0..100).map(|k| 8 * k + 2..8 * k + 5).collect();

    let painted = matches(&view, cx);
    let expected = parts(&view, &all, cx);
    // The first row's `c` and its line break, the second's `d`, and on
    // down the view.
    assert!(expected.len() >= 4, "{expected:?}");
    let (c, d) = (expected[0], expected[1]);
    assert_eq!((c.2 < d.2, c.0 > d.0), (true, true), "{c:?} {d:?}");
    assert!(c.1 > d.1, "{c:?} {d:?}");
    let got: Vec<_> = (painted.iter())
        .map(|found| (found.origin.x, found.size.width, found.origin.y))
        .collect();
    assert_eq!(got, expected);

    // Scrolled so the first row in view is a `def`: its match starts on the
    // row above, out of view, and its `d` still paints.
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    view.update(cx, |view, _| {
        view.scroll.y = px(line_height * 11.);
        view.autoscroll = false;
    });
    let painted = matches(&view, cx);
    let first = view.read_with(cx, |view, _| {
        let row = &view.layout.as_ref().unwrap().frame.rows[0];
        (row.buffer_row, row.start)
    });
    assert_eq!(first, (11, 8 * 5 + 4));
    let expected = parts(&view, &all, cx);
    let got: Vec<_> = (painted.iter())
        .map(|found| (found.origin.x, found.size.width, found.origin.y))
        .collect();
    assert_eq!(got, expected);
    // The first painted part is that `d`, on the first row in view.
    let top = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        layout.caret_bounds(8 * 5 + 4).unwrap().origin
    });
    assert_eq!((got[0].0, got[0].2), (top.x, top.y));
}

fn rects(painted: &[Bounds<Pixels>]) -> Vec<(Pixels, Pixels, Pixels)> {
    (painted.iter())
        .map(|found| (found.origin.x, found.size.width, found.origin.y))
        .collect()
}

#[gpui::test]
fn a_line_break_a_match_takes_in_paints_as_a_cell(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab\n\ncd", true);
    // Each line break, the blank row's and the one nvim matches after the
    // last row too.
    submit("/", "\\n", cx);
    let got = rects(&matches(&view, cx));
    let expected = parts(&view, &[2..3, 3..4, 6..7], cx);
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got, expected);
    let space = view.update(cx, |view, _| view.layout.as_mut().unwrap().space_width());
    assert!(got.iter().all(|cell| cell.1 == space), "{got:?}");

    // Over a blank row: `b` and its line break, the blank row's, `c`.
    submit("/", "b\\n\\nc", cx);
    let got = rects(&matches(&view, cx));
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got, parts(&view, &[1..5], cx));

    // The preview paints its match as the highlight does.
    cx.simulate_keystrokes("g g /");
    view.update(cx, |view, cx| view.preview_search("b\\n\\nc", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let current = view.read_with(cx, |view, _| rects(&view.painted_current_match));
    assert_eq!(current, parts(&view, &[1..5], cx));
}
