use std::time::{Duration, Instant};

use fe_buffer::{Point, ResolvedSelection};
use fe_command::Invocation;
use serde_json::json;

use crate::{HeadlessVim, VimState, context};

fn caret(offset: usize) -> ResolvedSelection {
    ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }
}

/// A buffer with carets at `points`.
fn vim(text: &str, points: &[(u32, u32)]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    let text = vim.model().text().clone();
    let carets = points
        .iter()
        .map(|&(row, column)| caret(text.point_to_offset(Point::new(row, column))));
    vim.model_mut().set_selections(carets);
    vim
}

/// Every caret, as a row and a byte column; each must be a caret.
fn carets(vim: &HeadlessVim) -> Vec<(u32, u32)> {
    let text = vim.model().text();
    vim.model()
        .selections()
        .iter()
        .map(|selection| {
            assert!(selection.range.is_empty(), "{selection:?} is a caret");
            let point = text.offset_to_point(selection.head());
            (point.row, point.column)
        })
        .collect()
}

/// No caret is past the last grapheme of a non-empty line.
fn on_graphemes(vim: &HeadlessVim) {
    let text = vim.model().text();
    for (row, column) in carets(vim) {
        let line = text.row_to_string(row);
        let last = unicode_segmentation::UnicodeSegmentation::grapheme_indices(line.as_str(), true)
            .next_back()
            .map_or(0, |(ix, _)| ix);
        assert!(
            column as usize <= last,
            "({row}, {column}) is past {line:?}'s last grapheme"
        );
    }
}

#[test]
fn carets_graphemes_and_goal_columns() {
    let mut two = vim("foo bar baz\none two three", &[(0, 0), (1, 0)]);
    two.feed("w").unwrap();
    assert_eq!(carets(&two), [(0, 4), (1, 4)]);
    two.feed("3l").unwrap();
    assert_eq!(carets(&two), [(0, 7), (1, 7)]);
    two.feed("$").unwrap();
    assert_eq!(carets(&two), [(0, 10), (1, 12)]);
    on_graphemes(&two);
    two.feed("l").unwrap();
    assert_eq!(carets(&two), [(0, 10), (1, 12)]);

    // j onto shorter lines keeps each caret on its line's last grapheme,
    // and the goal brings it back out.
    let mut short = vim(
        "abcdefgh\nab\nabcdefgh\nabcdefgh\nxy\nabcdefgh",
        &[(0, 6), (3, 5)],
    );
    short.feed("j").unwrap();
    assert_eq!(carets(&short), [(1, 1), (4, 1)]);
    on_graphemes(&short);
    short.feed("j").unwrap();
    assert_eq!(carets(&short), [(2, 6), (5, 5)]);

    // Each caret keeps its own goal: one caret's `l` failing keeps its goal
    // while the other's moves and resets.
    let mut own = vim("abcdefgh\nab\nabcdefgh\nabcdefgh", &[(0, 6), (2, 6)]);
    own.feed("jlk").unwrap();
    assert_eq!(carets(&own), [(0, 6), (2, 7)]);

    let mut combining = HeadlessVim::new("e\u{301}xy", (0, 0));
    combining.feed("l").unwrap();
    assert_eq!(combining.cursor(), (0, 3));
    combining.feed("h").unwrap();
    assert_eq!(combining.cursor(), (0, 0));

    let mut tab = HeadlessVim::new("abcdefgh\n\tabcd", (0, 6));
    assert_eq!(tab.model().tab_size(), 4);
    tab.feed("j").unwrap();
    // The tab covers display columns 0-3, then a, b, c: c covers 6.
    assert_eq!(tab.cursor(), (1, 3));
    let mut into_tab = HeadlessVim::new("abcdefgh\n\tabcd", (0, 2));
    into_tab.feed("j").unwrap();
    assert_eq!(into_tab.cursor(), (1, 0));

    let mut ends = HeadlessVim::new("short\nmuch longer line\n\nmid one\nx", (0, 0));
    ends.feed("$").unwrap();
    for end in [(1, 15), (2, 0), (3, 6), (4, 0)] {
        ends.feed("j").unwrap();
        assert_eq!(ends.cursor(), end);
    }
    ends.feed("gg").unwrap();
    assert_eq!(ends.cursor(), (0, 4));

    // A goal is dropped once the carets aren't where vim left them.
    let mut moved = HeadlessVim::new("abcdefgh\nab\nabcdefgh", (0, 6));
    moved.feed("j").unwrap();
    moved.model_mut().set_cursor(0);
    moved.feed("jj").unwrap();
    assert_eq!(moved.cursor(), (2, 0));

    // A selection collapses to its head first, whichever end that is.
    let mut selected = HeadlessVim::new("abcdef", (0, 0));
    selected.model_mut().select_range(1..4, false);
    selected.feed("l").unwrap();
    assert_eq!(carets(&selected), [(0, 5)]);
    selected.model_mut().select_range(1..4, true);
    selected.feed("l").unwrap();
    assert_eq!(carets(&selected), [(0, 2)]);
    // A caret left past the line end (as insert leaves it) moves from the
    // last grapheme.
    let mut past = HeadlessVim::new("abc\ndef", (0, 3));
    past.feed("h").unwrap();
    assert_eq!(past.cursor(), (0, 1));
}

#[test]
fn counts_are_bounded_by_the_text() {
    // The last word is one character, so its start is the last grapheme.
    for (motion, end) in [
        ("down", (2, 3)),
        ("word_forward", (2, 8)),
        ("right", (0, 8)),
    ] {
        let mut vim = HeadlessVim::new("first row\nsecond\nthe end x", (0, 3));
        let invocation = Invocation::of("vim.move")
            .unwrap()
            .args(json!({"motion": motion, "count": u32::MAX}))
            .context(context(&VimState::new()));
        let start = Instant::now();
        vim.dispatch(&invocation).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{motion} took {:?}",
            start.elapsed()
        );
        assert_eq!(vim.cursor(), end, "{motion}");
    }
}
