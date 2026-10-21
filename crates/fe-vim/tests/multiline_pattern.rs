//! Patterns that match across rows (`\n`, `\_x`, `\_.`, `\_[...]`, `\_^`,
//! `\_$`), parsed by `Pattern::parse_across` and found by
//! `Pattern::find_across`, against where nvim's search lands.
//!
//! The probes were made with nvim 0.12.4 (`nvim --clean --headless -l`):
//! the buffer's lines set, the cursor put at (row, column), the start from
//! `searchpos(pattern, 'nw')` (`'nwb'` going back), and the match's last
//! character from `searchpos(pattern, 'ncWe')` with the cursor on that
//! start. They are written below as `start (row, col) last (row, col)`,
//! rows and byte columns from 0. A last character at a row's length is its
//! line break, which nvim matches after the last row too, as if the text
//! ended in one: such a match ends one past the text's end here.

use std::ops::Range;
use std::time::{Duration, Instant};

use fe_text::Text;
use fe_vim::search::{Deadline, Interrupted, Pattern};

/// The offset of (row, column) in `text`; a column at the row's length is
/// its line break, and past the last row's, one past the text's end.
fn offset(text: &Text, (row, column): (u32, usize)) -> usize {
    text.row_start(row) + column
}

/// Where `find_across` lands for `pattern` on `buffer` from `cursor`.
fn find(pattern: &str, buffer: &str, cursor: (u32, usize), forward: bool) -> Option<Range<usize>> {
    let text = Text::new(buffer);
    let pattern =
        Pattern::parse_across(pattern, false).unwrap_or_else(|error| panic!("{pattern}: {error}"));
    pattern
        .find_across(&text, offset(&text, cursor), forward, &Deadline::never())
        .unwrap()
}

/// The range from nvim's start to just after its last character.
fn probed(buffer: &str, start: (u32, usize), last: (u32, usize)) -> Range<usize> {
    let text = Text::new(buffer);
    offset(&text, start)..offset(&text, last) + 1
}

const FORWARD: bool = true;
const BACK: bool = false;

#[test]
fn multiline_patterns_find_across_rows() {
    // (buffer, cursor, pattern, forward, nvim's start and last character,
    // or None for E486).
    type Case = (
        &'static str,
        (u32, usize),
        &'static str,
        bool,
        Option<((u32, usize), (u32, usize))>,
    );
    let cases: &[Case] = &[
        // `c\nd`: c at the end of a row, d starting the next. Going back
        // from the match's d; forward from its own start, round again.
        ("abc\ndef", (0, 0), r"c\nd", FORWARD, Some(((0, 2), (1, 0)))),
        ("abc\ndef", (1, 2), r"c\nd", BACK, Some(((0, 2), (1, 0)))),
        ("abc\ndef", (0, 2), r"c\nd", FORWARD, Some(((0, 2), (1, 0)))),
        // `\_s\+`: a run of blanks and line breaks.
        (
            "ab \n  cd",
            (0, 0),
            r"\_s\+",
            FORWARD,
            Some(((0, 2), (1, 1))),
        ),
        (
            "ab \n  cd\ne  f",
            (1, 4),
            r"\_s\+",
            FORWARD,
            Some(((2, 1), (2, 2))),
        ),
        (
            "ab \n  cd\ne  f",
            (1, 4),
            r"\_s\+",
            BACK,
            Some(((1, 0), (1, 1))),
        ),
        // `a\_.\{-}z` over three rows, forward, back and wrapping.
        (
            "xa1\n2\n3zq",
            (0, 0),
            r"a\_.\{-}z",
            FORWARD,
            Some(((0, 1), (2, 1))),
        ),
        (
            "xa1\n2\n3zq",
            (2, 2),
            r"a\_.\{-}z",
            BACK,
            Some(((0, 1), (2, 1))),
        ),
        (
            "xa1\n2\n3zq",
            (1, 0),
            r"a\_.\{-}z",
            FORWARD,
            Some(((0, 1), (2, 1))),
        ),
        // `\_[ab]`: a, b or a line break.
        ("xx\nab", (0, 0), r"\_[ab]", FORWARD, Some(((0, 2), (0, 2)))),
        ("xx\nab", (1, 0), r"\_[ab]", FORWARD, Some(((1, 1), (1, 1)))),
        ("xx\nab", (1, 1), r"\_[ab]", BACK, Some(((1, 0), (1, 0)))),
        ("xx\nab", (1, 0), r"\_[ab]", BACK, Some(((0, 2), (0, 2)))),
        // `\n\n` over an empty row; three line breaks in a row aren't there.
        ("a\n\nb", (0, 0), r"\n\n", FORWARD, Some(((0, 1), (1, 0)))),
        ("a\n\nb", (2, 0), r"\n\n", BACK, Some(((0, 1), (1, 0)))),
        ("a\n\nb", (0, 0), r"\n\n\n", FORWARD, None),
        ("a\n\nb", (0, 0), r"\n\n\n", BACK, None),
        // `x\_$`: an x ending a row, wrapping both ways.
        (
            "ax\nxb\nx",
            (0, 0),
            r"x\_$",
            FORWARD,
            Some(((0, 1), (0, 1))),
        ),
        (
            "ax\nxb\nx",
            (0, 1),
            r"x\_$",
            FORWARD,
            Some(((2, 0), (2, 0))),
        ),
        (
            "ax\nxb\nx",
            (2, 0),
            r"x\_$",
            FORWARD,
            Some(((0, 1), (0, 1))),
        ),
        ("ax\nxb\nx", (0, 1), r"x\_$", BACK, Some(((2, 0), (2, 0)))),
        ("ax\nxb\nx", (2, 0), r"x\_$", BACK, Some(((0, 1), (0, 1)))),
        // No match: a line break isn't where these want it.
        ("abc\ndef", (0, 0), r"f\na", FORWARD, None),
        ("abc\ndef", (0, 0), r"c\ne", FORWARD, None),
        ("abc\ndef", (0, 0), r"c\ne", BACK, None),
        // A match before the cursor on its row that ends on a later row
        // ends the row's matches.
        (
            "ab\nab\nab",
            (1, 1),
            r"b\na",
            FORWARD,
            Some(((0, 1), (1, 0))),
        ),
        ("ab\nab\nab", (1, 1), r"b\na", BACK, Some(((0, 1), (1, 0)))),
        ("ab\nab\nab", (0, 0), r"b\na", BACK, Some(((1, 1), (2, 0)))),
        ("a\nb\nc", (0, 0), r"\_.*", FORWARD, Some(((1, 0), (2, 1)))),
        ("a\nb\nc", (1, 0), r"\_.*", FORWARD, Some(((2, 0), (2, 1)))),
        // The last row's line break, which nvim matches as if the text
        // ended in one.
        ("abc", (0, 0), r"c\n", FORWARD, Some(((0, 2), (0, 3)))),
        ("a\nb", (1, 0), r"\n", FORWARD, Some(((0, 1), (0, 1)))),
        ("a\nb", (0, 0), r"\n", BACK, Some(((1, 1), (1, 1)))),
        ("a\n", (0, 0), r"\n\n", FORWARD, Some(((0, 1), (1, 0)))),
        ("a\n", (1, 0), r"\n\n", BACK, Some(((0, 1), (1, 0)))),
        ("aaa\naaa", (0, 0), r"a\n", BACK, Some(((1, 2), (1, 3)))),
        ("aaa\naaa", (1, 1), r"a\n", BACK, Some(((0, 2), (0, 3)))),
        (
            "a\nb",
            (0, 0),
            r"\_.\_.\_.\_.",
            FORWARD,
            Some(((0, 0), (1, 1))),
        ),
        // A line break at a row's end counts as on its last character.
        ("a b\nc", (0, 0), r"\_s", FORWARD, Some(((0, 1), (0, 1)))),
        ("a b\nc", (0, 1), r"\_s", FORWARD, Some(((0, 3), (0, 3)))),
        (
            "a\nb\nc",
            (0, 0),
            r"[x\n]\+",
            FORWARD,
            Some(((1, 1), (1, 1))),
        ),
        // Collections: one without `\n` or `\_` stays within a row, as
        // does `[^...\n]`; `\_[^...]` doesn't.
        (
            "a\nb\nc",
            (0, 0),
            r"[^x]\+",
            FORWARD,
            Some(((1, 0), (1, 0))),
        ),
        (
            "a\nb\nc",
            (0, 0),
            r"\_[^x]\+",
            FORWARD,
            Some(((1, 0), (2, 1))),
        ),
        (
            "a\tb",
            (0, 0),
            r"[\x01-\x7f]\+",
            FORWARD,
            Some(((0, 0), (0, 2))),
        ),
        (
            "a b\nc",
            (0, 0),
            r"[[:space:]]\+",
            FORWARD,
            Some(((0, 1), (0, 1))),
        ),
        ("a\nb", (0, 0), r"\_x", FORWARD, Some(((1, 0), (1, 0)))),
        ("xay\nz", (0, 0), r"\_S\_s", FORWARD, Some(((0, 2), (0, 3)))),
        ("a\nb\nc", (0, 0), r"\_S\+", FORWARD, Some(((1, 0), (2, 1)))),
        // `^` after `\n` and `$` before it are a row's start and end, but
        // for a very nomagic `^`; and `\_^` is one anywhere.
        (
            "ab\ncd",
            (0, 0),
            r"b\n\_^c",
            FORWARD,
            Some(((0, 1), (1, 0))),
        ),
        (
            "ab\ncd",
            (0, 0),
            r"\Mb\n^c",
            FORWARD,
            Some(((0, 1), (1, 0))),
        ),
        ("ab\ncd", (0, 0), r"\Vb\n^c", FORWARD, None),
        (
            "ab\n^cd",
            (0, 0),
            r"b\n\^c",
            FORWARD,
            Some(((0, 1), (1, 1))),
        ),
        ("ab\ncd", (0, 0), r"b$\nc", FORWARD, Some(((0, 1), (1, 0)))),
        ("a\nb\nc", (0, 0), r"a$\nb", FORWARD, Some(((0, 0), (1, 0)))),
        ("a\nb\nc", (0, 0), r"\n^b", FORWARD, Some(((0, 1), (1, 0)))),
        // `\zs` across a line break, and groups around one.
        ("ab\nc", (0, 0), r"b\zs\nc", FORWARD, Some(((0, 2), (1, 0)))),
        (
            "ab\nc",
            (0, 0),
            r"\v(a|b)\n",
            FORWARD,
            Some(((0, 1), (0, 2))),
        ),
        // A repeat whose ways end differently, matched by backtracking.
        (
            "ab\nab\nab",
            (0, 0),
            r"\%(a\|\_.\)*b",
            FORWARD,
            Some(((1, 0), (2, 1))),
        ),
    ];
    for &(buffer, cursor, pattern, forward, nvim) in cases {
        let want = nvim.map(|(start, last)| probed(buffer, start, last));
        assert_eq!(
            find(pattern, buffer, cursor, forward),
            want,
            "{pattern} from {cursor:?} {} on {buffer:?}",
            if forward { "forward" } else { "back" }
        );
    }
    // Zero-width: `b\n\zsc` starts on (1, 0) (its last character is c),
    // and `\_^` from (0, 0) on "a", "b", "c" lands on (1, 0).
    assert_eq!(find(r"b\n\zsc", "ab\nc", (0, 0), FORWARD), Some(3..4));
    assert_eq!(find(r"\_^", "a\nb\nc", (0, 0), FORWARD), Some(2..2));
}

/// Matches whose rows are read in more than one window: past the rows one
/// holds, a row's matches are told again from the next, and going back
/// windows are listed from the last.
#[test]
fn multiline_patterns_span_windows() {
    let mut rows = vec!["."; 2000];
    rows[700] = "x";
    rows[703] = "y";
    rows[1500] = "x";
    rows[1501] = "y";
    let buffer = rows.join("\n");
    let row = |row: usize| 2 * row;
    for (pattern, from, forward, want) in [
        (r"x\n.\n.\ny", 0, FORWARD, row(700)..row(703) + 1),
        (r"x\n.\n.\ny", row(1999), BACK, row(700)..row(703) + 1),
        (r"x\_.\{-}y", row(800), FORWARD, row(1500)..row(1501) + 1),
        (r"x\_.\{-}y", row(800), BACK, row(700)..row(703) + 1),
        (
            r"x\n\_.\{-1,4}y",
            row(1600),
            FORWARD,
            row(700)..row(703) + 1,
        ),
        (r"x\n\_.\{-1,4}y", row(1600), BACK, row(700)..row(703) + 1),
        (r"x\ny", row(1600), BACK, row(1500)..row(1501) + 1),
    ] {
        let text = Text::new(&buffer);
        let parsed = Pattern::parse_across(pattern, false).unwrap();
        assert_eq!(
            parsed.find_across(&text, from, forward, &Deadline::never()),
            Ok(Some(want)),
            "{pattern} from {from} forward {forward}"
        );
    }
}

/// A pattern without any atom that spans rows is found as `find` finds it,
/// and `parse` still refuses the atoms that do.
#[test]
fn single_row_patterns_keep_their_search() {
    let pattern = Pattern::parse_across("b", false).unwrap();
    assert!(!pattern.spans_rows());
    assert!(Pattern::parse_across(r"b\n", false).unwrap().spans_rows());
    assert!(Pattern::parse_across(r"x\_$", false).unwrap().spans_rows());
    let text = Text::new("ab\nab");
    assert_eq!(
        pattern.find_across(&text, 1, true, &Deadline::never()),
        Ok(pattern.find(&text, 1, true))
    );
    for atom in [r"\n", r"\_s", r"\_.", r"\_[a]", r"\_^", r"\_$"] {
        assert!(Pattern::parse(&format!("a{atom}")).is_err(), "{atom}");
    }
}

#[test]
fn multiline_pattern_is_interrupted() {
    // Every way of taking each `a` (as `a` or as `\_.`) is tried before
    // the missing `b` fails a start, over rows that `\_.` runs across.
    let buffer = vec!["a".repeat(40); 40].join("\n");
    let text = Text::new(&buffer);
    let pattern = Pattern::parse_across(r"\%(a\|\_.\)*b", false).unwrap();
    let limit = Duration::from_millis(100);
    let deadline = Deadline::after(limit);
    let started = Instant::now();
    assert_eq!(
        pattern.find_across(&text, 0, true, &deadline),
        Err(Interrupted)
    );
    assert!(deadline.ran_out());
    // Stopped by its own deadline: the clock is read every few thousand
    // steps, well within a second of passing it, even on a loaded machine.
    let took = started.elapsed();
    assert!(took < limit + Duration::from_secs(5), "took {took:?}");

    // One translated to a regex stops at a deadline already passed, before
    // reading a row, either way.
    let pattern = Pattern::parse_across(r"c\nd", false).unwrap();
    for forward in [true, false] {
        let deadline = Deadline::after(Duration::ZERO);
        assert_eq!(
            pattern.find_across(&Text::new("abc\ndef"), 0, forward, &deadline),
            Err(Interrupted)
        );
    }
}
