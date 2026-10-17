//! `[(` `[{` `])` `]}` (`:help [(`), a feature module; see
//! [`crate::feature`].
//!
//! After `[` the keys `(` and `{` go back to the `count`th unmatched `(` or
//! `{`; after `]` the keys `)` and `}` go forward to the `count`th unmatched
//! `)` or `}`. Exclusive and charwise, a jump, across rows. Brackets in
//! double-quoted strings are skipped as `%` skips them, from outside one
//! (from inside one, only that string counts), and so are escaped ones and
//! those quoted as chars. With fewer than `count` unmatched it goes as
//! far as there are; with none it fails and sets no jump mark.

use crate::{
    Pending, Step, VimKey,
    args::Motion,
    feature::Feature,
    motion::{Cursor, PAIRS, Quotes, Rules, Scan, nth_unmatched},
};

pub(crate) const FEATURE: Feature = Feature {
    bracket: Some(bracket),
    ..Feature::NONE
};

/// `(` `{` after `[`, `)` `}` after `]`.
fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    let brace = match (forward, key) {
        (false, VimKey::Char('(')) | (true, VimKey::Char(')')) => false,
        (false, VimKey::Char('{')) | (true, VimKey::Char('}')) => true,
        _ => return None,
    };
    Some(pending.clone().motion(Motion::Unmatched { brace, forward }))
}

/// Where the `count`th unmatched bracket lies from the caret at `offset`;
/// a count past the last one goes as far as there are, as nvim's does, and
/// none at all fails.
pub(crate) fn find(
    text: &fe_text::Text,
    offset: usize,
    brace: bool,
    forward: bool,
    count: u32,
) -> Option<usize> {
    let pair = PAIRS[if brace { 2 } else { 0 }];
    let mut at = None;
    for _ in 0..count.max(1) {
        let from = at.unwrap_or(offset);
        let row = text.offset_to_point(from).row;
        let scan = Scan::new(text, row);
        let col = from - scan.start;
        let quoted = scan.quoted.get(col).copied().unwrap_or(false);
        // On a string's opening quote, going forward, the string is the one
        // started in.
        let opening =
            forward && !quoted && scan.bytes.get(col) == Some(&b'"') && !scan.escaped[col];
        let rules = Rules {
            escaped: false,
            quotes: Quotes::from(quoted || opening),
            literals: true,
        };
        match nth_unmatched(text, from, forward, pair, rules, 1) {
            Some(next) => at = Some(next),
            None => break,
        }
    }
    at
}

/// [`Motion::Unmatched`] as a move: the caret goes to the bracket, or the
/// motion fails.
pub(crate) fn motion(
    cursor: &mut Cursor<'_>,
    brace: bool,
    forward: bool,
    count: u32,
    goal: Option<usize>,
) -> Result<Option<usize>, Option<usize>> {
    match find(cursor.text, cursor.offset(), brace, forward, count) {
        Some(offset) => {
            cursor.goto(offset);
            Ok(None)
        }
        None => Err(goal),
    }
}
