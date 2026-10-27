//! `]]` `[[` `][` `[]` (`:help ]]`, `:help section`), a feature module; see
//! [`crate::feature`].
//!
//! After `]` the keys `]` and `[` go forward to the `count`th next row that
//! starts a section (`]]`) or ends one (`][`); after `[` the keys `[` and
//! `]` go back to the `count`th previous one (`[[` and `[]`). A row starts
//! a section with a `{` in its first column, and ends one with a `}`
//! there; for both, a row starting with a formfeed or with one of the
//! nroff section macros of nvim's default 'sections' (`.SH`, `.NH`, `.H`,
//! `.HU`, `.nh`, `.sh`) counts too. Charwise and exclusive, and a jump.
//! Probed in nvim 0.12.4 (`--clean`), which moves as Vim's `findpar` does:
//!
//! - The row the caret is on never counts, so from a `{` row `]]` goes on
//!   to the next one and `[[` back to the one before.
//! - Each step goes to the next such row, or else stops at the text's last
//!   row (first, going back). A step before the last that runs out of text
//!   fails the motion: nothing moves and the jump mark stays. So a count of
//!   one never fails, even with no `{` at all (Python, an empty buffer):
//!   `]]` goes to the last row, `[[` to the first; `2]]` there fails, and a
//!   macro running it stops.
//! - In normal mode and the visual modes the caret goes to the row's first
//!   non-blank (its last char on a row of blanks), as `beginline` puts it;
//!   on a `{` or `}` row, a formfeed one or a macro's that's column 0.
//! - After an operator it goes to column 0, so `d]]` from a row's first
//!   column takes whole rows (`:help exclusive-linewise`), except at the
//!   text's last row going forward with `]]`: there it takes the row's last
//!   char too, as nvim's becomes inclusive, even on a `{` row.
//! - After an operator `]]` also stops at a `}` in the first column (Vi's
//!   quirk the help keeps) and then ends at the start of the row after it,
//!   so `d]]` inside a function takes its closing `}` row. A `}` on the
//!   text's last row ends there, at its column 0. Neither a visual `]]` nor
//!   `[[` stops at a `}`.
//! - Each sets the jump mark (`''`) when it moves, and when it gets
//!   nowhere (`[[` from the first column of row 0), but not when it fails.
//! - Each resets the goal column, even when it fails, after an operator
//!   too: `$j:norm! d5]]` then `j` goes to the next row's column 2, not
//!   its end.
//!
//! Limits: 'sections' can't be set (it's nvim's default), and from inside
//! a closed fold nvim starts after it (before it, going back) where this
//! starts from the caret's row.

use fe_text::Text;

use crate::{Pending, Step, VimKey, args::Motion, feature::Feature, motion::Cursor};

pub(crate) const FEATURE: Feature = Feature {
    bracket: Some(bracket),
    ..Feature::NONE
};

/// nvim's default 'sections': pairs of macro names, a space matching a
/// space after the name or the row's end.
const SECTIONS: &[u8; 12] = b"SHNHH HUnhsh";

/// `]` `[` after `]`, and `[` `]` after `[`.
fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    let close = match (forward, key) {
        (true, VimKey::Char(']')) | (false, VimKey::Char('[')) => false,
        (true, VimKey::Char('[')) | (false, VimKey::Char(']')) => true,
        _ => return None,
    };
    Some(pending.clone().motion(Motion::Section { forward, close }))
}

/// Whether `row` is where a section motion stops: one starting with `{`
/// (`}` if `close`), a formfeed or a 'sections' macro, or with `}` too if
/// `both` (`]]` after an operator).
fn stops(text: &Text, row: u32, close: bool, both: bool) -> bool {
    let (head, len) = head(text, row);
    let head = &head[..len];
    match head.first() {
        Some(b'{') => !close,
        Some(b'}') => close || both,
        Some(b'\x0c') => true,
        Some(b'.') => macro_row(&head[1..]),
        _ => false,
    }
}

/// The first three bytes of `row`, and how many it has of them.
fn head(text: &Text, row: u32) -> ([u8; 3], usize) {
    let start = text.row_start(row);
    let end = start + text.row_len(row) as usize;
    let mut head = ([0; 3], 0);
    for byte in text
        .chunks_in_range(start..end)
        .flat_map(str::bytes)
        .take(3)
    {
        head.0[head.1] = byte;
        head.1 += 1;
    }
    head
}

/// Whether `name` (what follows a row's leading `.`, at most two bytes of
/// it) is one of [`SECTIONS`], as Vim's `inmacro` matches.
fn macro_row(name: &[u8]) -> bool {
    let (first, second) = (name.first().copied(), name.get(1).copied());
    SECTIONS.chunks(2).any(|pair| {
        let ends = |byte: Option<u8>| matches!(byte, None | Some(b' '));
        let first_matches = Some(pair[0]) == first || pair[0] == b' ' && ends(first);
        let second_matches =
            Some(pair[1]) == second || pair[1] == b' ' && (first.is_none() || ends(second));
        first_matches && second_matches
    })
}

/// The row `count` section rows from `row`: each step goes on to the next
/// row [`stops`] takes (the one it starts on doesn't count), or stops at
/// the text's last row (first, going back). `None` if a step before the
/// last runs out of text.
fn section(
    text: &Text,
    row: u32,
    forward: bool,
    close: bool,
    both: bool,
    count: u32,
) -> Option<u32> {
    let last_row = text.row_count() - 1;
    let mut row = row;
    for step in 1..=count.max(1) {
        loop {
            let at_end = match forward {
                true => row == last_row,
                false => row == 0,
            };
            if at_end {
                if step < count {
                    return None;
                }
                break;
            }
            row = match forward {
                true => row + 1,
                false => row - 1,
            };
            if stops(text, row, close, both) {
                break;
            }
        }
    }
    Some(row)
}

/// Where the motion goes from the caret at `offset`, `count` times: the
/// row, and whether an operator takes that row's last char (it ran on to
/// the text's last row with `]]`). `op` is after an operator, where `]]`
/// stops at a `}` too and ends on the row after it.
fn find(
    text: &Text,
    offset: usize,
    forward: bool,
    close: bool,
    op: bool,
    count: u32,
) -> Option<(u32, bool)> {
    let both = op && forward && !close;
    let from = text.offset_to_point(offset).row;
    let mut row = section(text, from, forward, close, both, count)?;
    let last_row = text.row_count() - 1;
    if both && head(text, row).0[0] == b'}' {
        // On the last row Vim's row past the text is put back on it.
        if row == last_row {
            return Some((row, false));
        }
        row += 1;
    }
    Some((row, forward && !close && row == last_row))
}

/// [`Motion::Section`] as a move: the caret goes to the row's first
/// non-blank, or for an operator to its column 0 (past its last char at
/// the text's end with `]]`), or the motion fails. Either way the goal
/// column is reset, as nvim's is.
pub(crate) fn motion(
    cursor: &mut Cursor<'_>,
    forward: bool,
    close: bool,
    count: u32,
) -> Result<Option<usize>, Option<usize>> {
    let text = cursor.text;
    let (row, to_end) =
        find(text, cursor.offset(), forward, close, cursor.op, count).ok_or(None)?;
    cursor.goto(text.row_start(row));
    match (cursor.op, to_end) {
        (true, true) => cursor.ix = cursor.line.len(),
        (true, false) => cursor.ix = 0,
        (false, _) => cursor.go_to_first_non_blank(),
    }
    Ok(None)
}

/// Whether the motion (`count` times, after an operator if `op`) from the
/// caret at `offset` gets anywhere, as a jump must to set the jump mark.
pub(crate) fn lands(
    text: &Text,
    offset: usize,
    forward: bool,
    close: bool,
    op: bool,
    count: Option<u32>,
) -> bool {
    find(text, offset, forward, close, op, count.unwrap_or(1)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = "int a;\n\nint f()\n{\n    x;\n}\n\nint g()\n{\n    y;\n}\n// end";

    fn text(source: &str) -> Text {
        Text::new(source)
    }

    #[test]
    fn a_count_lands_until_a_step_before_the_last_runs_out() {
        let text = text(C);
        // Two `{` rows after row 0, then the last row.
        for (count, lands_on) in [(1, true), (2, true), (3, true), (4, false), (9, false)] {
            assert_eq!(
                lands(&text, 0, true, false, false, Some(count)),
                lands_on,
                "{count}]]"
            );
        }
        let end = text.len();
        for (count, lands_on) in [(1, true), (3, true), (4, false)] {
            assert_eq!(
                lands(&text, end, false, false, false, Some(count)),
                lands_on,
                "{count}[["
            );
        }
    }

    #[test]
    fn without_braces_one_step_lands_and_two_fail() {
        let text = text("def f():\n    pass\n\ndef g():\n    return 1");
        for (forward, close) in [(true, false), (false, false), (true, true), (false, true)] {
            let from = if forward { 0 } else { text.len() };
            assert!(lands(&text, from, forward, close, false, None));
            assert!(!lands(&text, from, forward, close, false, Some(2)));
        }
        assert!(lands(&self::text(""), 0, true, false, false, None));
        assert!(!lands(&self::text(""), 0, false, false, false, Some(2)));
    }

    #[test]
    fn after_an_operator_a_close_brace_is_a_step_too() {
        // From row 0: `{` (1), `}` (2), the last row (3).
        let text = text("a\n{\n}\nb");
        assert!(!lands(&text, 0, true, false, false, Some(3)));
        assert!(lands(&text, 0, true, false, true, Some(3)));
        assert_eq!(find(&text, 0, true, false, true, 2), Some((3, true)));
        // Only `]]` stops at both.
        assert!(!lands(&text, 0, true, true, true, Some(3)));
        let end = text.len();
        assert!(!lands(&text, end, false, false, true, Some(3)));
    }

    #[test]
    fn a_close_brace_on_the_last_row_ends_there() {
        let text = text("a\nb\n}");
        assert_eq!(find(&text, 0, true, false, true, 1), Some((2, false)));
        assert_eq!(find(&text, 0, true, false, false, 1), Some((2, true)));
    }

    #[test]
    fn formfeeds_and_section_macros_stop_both_ways() {
        let rows = [
            ("\x0cb", true),
            (".SH x", true),
            (".NH", true),
            (".H", true),
            (".H x", true),
            (".HU", true),
            (".HUX", true),
            (".nh", true),
            (".sh", true),
            (".", false),
            (".PP", false),
            (".Hx", false),
            (" {", false),
            ("x", false),
        ];
        for (row, stop) in rows {
            let text = text(&format!("a\n{row}\nb"));
            for close in [false, true] {
                assert_eq!(stops(&text, 1, close, false), stop, "{row:?}");
            }
        }
    }
}
