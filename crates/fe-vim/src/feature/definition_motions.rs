//! `]m` `[m` `]M` `[M`: to the next or previous start or end of a function,
//! from the host's syntax, as nvim-treesitter-textobjects' `@function.outer`
//! motions (`goto_next_start` and the like) go, a feature module; see
//! [`crate::feature`].
//!
//! The host lists every function and method by start
//! ([`EditorHost::function_bounds`]). `motion::motion` sees only the text,
//! so [`line_motions::with_view`] asks for the list before a command with
//! one of these motions runs, and only then. Without syntax there are none.
//!
//! After `]` the key `m` goes to the `count`th function start after the
//! caret, and `M` to the `count`th function's last char after it; after
//! `[` they go back to the `count`th before it. A function nested in
//! another counts as any other: from inside `fn a`, `]m` goes to the start
//! of `fn inner` within it, and `[M` doesn't stop at `fn a`'s end, which
//! is ahead. A row's line break ending a function's node isn't its last
//! char; the char before is. With fewer than `count` it goes as far as
//! there are, as nvim's does; with none it fails (stopping a macro) and
//! sets no jump mark.
//!
//! Charwise; `]m` `[m` exclusive and `]M` `[M` inclusive, so `d]M` takes
//! the closing brace, and `d[M` the caret's char too. In a visual mode the
//! vim cursor goes there. A jump.
//!
//! [`EditorHost::function_bounds`]: fe_editor::commands::EditorHost::function_bounds

use std::ops::Range;

use fe_text::{Bias, Text};

use crate::{
    Pending, Step, VimKey,
    args::{Motion, RangeClass},
    feature::Feature,
    line_motions,
    motion::Cursor,
};

pub(crate) const FEATURE: Feature = Feature {
    bracket: Some(bracket),
    ..Feature::NONE
};

/// `m` and `M` after `[` or `]`.
fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    let end = match key {
        VimKey::Char('m') => false,
        VimKey::Char('M') => true,
        _ => return None,
    };
    Some(pending.clone().motion(Motion::Function { forward, end }))
}

/// [`Motion::Function`]'s class as an operator's target: inclusive to an
/// end, exclusive to a start.
pub(crate) fn class(end: bool) -> RangeClass {
    match end {
        true => RangeClass::Inclusive,
        false => RangeClass::Exclusive,
    }
}

/// The offset of the last char of a function whose node is `range` (not
/// empty): the one before its end, or before the line break it ends with.
fn last_char(text: &Text, range: &Range<usize>) -> usize {
    let last = text.clip_offset(range.end - 1, Bias::Left);
    match last > range.start && text.char_at(last) == Some('\n') {
        true => text.clip_offset(last - 1, Bias::Left),
        false => last,
    }
}

/// Where the motion goes from the caret at `offset` among `functions`:
/// the `count`th start (an `end`'s last char) after it (`forward`) or
/// before it, or the furthest there is short of that; `None` if there's
/// none.
pub(crate) fn find(
    text: &Text,
    functions: &[Range<usize>],
    offset: usize,
    forward: bool,
    end: bool,
    count: u32,
) -> Option<usize> {
    let mut stops: Vec<usize> = (functions.iter())
        .filter(|range| !range.is_empty() && range.end <= text.len())
        .map(|range| match end {
            true => last_char(text, range),
            false => range.start,
        })
        .collect();
    // Starts come sorted, but a nested function ends before the one around.
    stops.sort_unstable();
    stops.dedup();
    let n = count.max(1) as usize;
    match forward {
        true => {
            let ahead = &stops[stops.partition_point(|&stop| stop <= offset)..];
            ahead.get(n - 1).or(ahead.last()).copied()
        }
        false => {
            let behind = &stops[..stops.partition_point(|&stop| stop < offset)];
            let at = behind.len().saturating_sub(n);
            behind.get(at).copied()
        }
    }
}

/// [`Motion::Function`] as a move: the caret goes to the function's start
/// or last char, or the motion fails, keeping the goal column.
pub(crate) fn motion(
    cursor: &mut Cursor<'_>,
    forward: bool,
    end: bool,
    count: u32,
    goal: Option<usize>,
) -> Result<Option<usize>, Option<usize>> {
    let offset = cursor.offset();
    let found = line_motions::functions(|functions| {
        find(cursor.text, functions, offset, forward, end, count)
    });
    match found {
        Some(offset) => {
            cursor.goto(offset);
            Ok(None)
        }
        None => Err(goal),
    }
}

/// Whether the motion from the caret at `offset` gets anywhere, as a jump
/// must to set the jump mark.
pub(crate) fn lands(text: &Text, offset: usize, forward: bool, end: bool) -> bool {
    let offset = Cursor::at(text, offset).offset();
    line_motions::functions(|functions| find(text, functions, offset, forward, end, 1)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "fn a() {\n    fn b() {}\n}\n\nfn c() {\n}\n";

    fn at(needle: &str) -> usize {
        CODE.find(needle).unwrap()
    }

    fn functions() -> Vec<Range<usize>> {
        vec![
            at("fn a")..at("}\n\n") + 1,
            at("fn b")..at("{}") + 2,
            at("fn c")..CODE.len() - 1,
        ]
    }

    fn go(from: &str, forward: bool, end: bool, count: u32) -> Option<usize> {
        let text = Text::new(CODE);
        find(&text, &functions(), at(from), forward, end, count)
    }

    #[test]
    fn starts_and_ends_nested_ones_counting() {
        assert_eq!(go("fn a", true, false, 1), Some(at("fn b")));
        assert_eq!(go("fn a", true, false, 2), Some(at("fn c")));
        // Short of the count, as far as there are.
        assert_eq!(go("fn a", true, false, 9), Some(at("fn c")));
        assert_eq!(go("fn c", true, false, 1), None);
        assert_eq!(go("fn c", false, false, 1), Some(at("fn b")));
        assert_eq!(go("fn c", false, false, 2), Some(at("fn a")));
        assert_eq!(go("fn a", false, false, 1), None);
        // Ends: `fn b`'s comes before `fn a`'s.
        assert_eq!(go("fn a", true, true, 1), Some(at("{}") + 1));
        assert_eq!(go("fn a", true, true, 2), Some(at("}\n\n")));
        assert_eq!(go("fn c", false, true, 1), Some(at("}\n\n")));
        assert_eq!(go("fn c", false, true, 3), Some(at("{}") + 1));
        assert_eq!(go("fn a", false, true, 1), None);
    }

    #[test]
    fn a_line_break_ending_a_node_is_not_its_last_char() {
        let text = Text::new("fn a() {\n}\nx");
        let functions = [Range { start: 0, end: 11 }];
        assert_eq!(find(&text, &functions, 0, true, true, 1), Some(9));
        // Nor is a node past the text taken.
        assert_eq!(
            find(&text, &[Range { start: 0, end: 99 }], 0, true, true, 1),
            None
        );
        assert_eq!(
            find(&text, &[Range { start: 0, end: 0 }], 0, true, false, 1),
            None
        );
    }
}
