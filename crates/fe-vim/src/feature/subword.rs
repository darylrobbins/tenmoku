//! Subword motions, as CamelCaseMotion, vim-wordmotion and nvim-spider give
//! them: while [`VimState::set_subword_motions`] is on, `w b e ge` step
//! through the parts of a keyword run (`fooBar`, `HTTPServer`, `snake_case`,
//! `utf8Decode`) as they step through words, under operators, counts, the
//! visual modes and `.` alike. `W B E gE` still move by WORDs. Off (the
//! default), the reducer gives the word motions; `vim.move` and
//! `vim.operate` take [`Motion::SubwordForward`] and the rest either way.
//!
//! Inside a run of keyword chars (fe-vim's word class), a subword ends
//! - between a lowercase letter and an uppercase one (`foo|Bar`);
//! - before the last of a run of uppercase letters that a lowercase one
//!   follows (`HTTP|Server`);
//! - between a letter and a digit, either way (`utf|8|Decode`);
//! - around each run of `_`, which belongs to no subword: the motions pass
//!   over it as over blanks (`w` on `snake_case` stops on `c`). A keyword
//!   run of only `_` is one subword.
//!
//! Letter case is Unicode's. Non-keyword runs, blanks, empty rows and row
//! ends are as for the word motions, which these are, stepping by another
//! [`Granularity`](crate::motion::Granularity): `motion.rs`'s
//! [`Cursor::unit`] and [`Cursor::splits`] read the rules here.
//!
//! [`Motion::SubwordForward`]: crate::Motion::SubwordForward

use crate::{
    VimState,
    motion::{Class, Cursor, Line, class},
};

/// nvim's `charclass()` for a keyword char.
const KEYWORD: u32 = 2;

impl VimState {
    /// Makes `w b e ge` step by subwords ([`crate::feature::subword`]), in
    /// normal mode, the visual modes and after an operator. Off by default;
    /// `W B E gE` move by WORDs either way.
    pub fn set_subword_motions(&mut self, on: bool) {
        self.subword_motions = on;
    }
}

/// The base char of grapheme `ix` of `line`, if it has one and it's a
/// keyword char.
fn keyword(line: &Line, ix: usize) -> Option<char> {
    if ix >= line.len() {
        return None;
    }
    let grapheme = line.grapheme(ix);
    match class(grapheme, false) {
        Class::Of(KEYWORD) => grapheme.chars().next(),
        _ => None,
    }
}

/// Whether one subword ends and the next starts between graphemes `ix - 1`
/// and `ix` of `line`, both keyword chars.
pub(crate) fn splits(line: &Line, ix: usize) -> bool {
    let prev = ix.checked_sub(1).and_then(|ix| keyword(line, ix));
    let (Some(prev), Some(next)) = (prev, keyword(line, ix)) else {
        return false;
    };
    let digit = |ch: char| ch.is_numeric() && !ch.is_alphabetic();
    let letter_then_digit = |a: char, b: char| a.is_alphabetic() && digit(b);
    (prev == '_') != (next == '_')
        || prev.is_lowercase() && next.is_uppercase()
        || prev.is_uppercase()
            && next.is_uppercase()
            && keyword(line, ix + 1).is_some_and(char::is_lowercase)
        || letter_then_digit(prev, next)
        || letter_then_digit(next, prev)
}

/// Whether grapheme `ix` of the cursor's row is in a run of `_` that a
/// keyword run holds more than, which subwords pass over as blanks. The run
/// is measured once ([`Cursor::underscores`]), so that stepping along it
/// takes no longer than the run.
pub(crate) fn passed_over(cursor: &Cursor, ix: usize) -> bool {
    let line = &cursor.line;
    if keyword(line, ix) != Some('_') {
        return false;
    }
    let at = |ix: usize| line.start + line.starts.get(ix).copied().unwrap_or(line.text.len());
    if let Some((start, end, passed)) = cursor.underscores.get()
        && (start..end).contains(&at(ix))
    {
        return passed;
    }
    let mut first = ix;
    while first > 0 && keyword(line, first - 1) == Some('_') {
        first -= 1;
    }
    let mut end = ix + 1;
    while keyword(line, end) == Some('_') {
        end += 1;
    }
    let passed = first
        .checked_sub(1)
        .and_then(|ix| keyword(line, ix))
        .is_some()
        || keyword(line, end).is_some();
    cursor.underscores.set(Some((at(first), at(end), passed)));
    passed
}

#[cfg(test)]
mod tests {
    use fe_text::Text;

    use super::*;

    /// Where `w`'s subword boundaries fall in `row`: the indices `splits`
    /// takes, and the ones `passed_over` passes.
    fn parts(row: &str) -> (Vec<usize>, Vec<usize>) {
        let text = Text::new(row);
        let cursor = Cursor::at(&text, 0);
        let len = cursor.line.len();
        let split = (0..=len + 1)
            .filter(|&ix| splits(&cursor.line, ix))
            .collect();
        let passed = (0..=len + 1)
            .filter(|&ix| passed_over(&cursor, ix))
            .collect();
        (split, passed)
    }

    #[test]
    fn the_rules_split_keyword_runs() {
        assert_eq!(parts("fooBarBaz").0, [3, 6]);
        assert_eq!(parts("HTTPServer").0, [4]);
        assert_eq!(parts("HTTPS").0, Vec::<usize>::new());
        assert_eq!(parts("ABc").0, [1]);
        assert_eq!(parts("utf8Decode").0, [3, 4]);
        assert_eq!(parts("x86_64").0, [1, 3, 4]);
        assert_eq!(parts("snake_case"), (vec![5, 6], vec![5]));
        assert_eq!(parts("__init__"), (vec![2, 6], vec![0, 1, 6, 7]));
        // Unicode's case: `é|A|Éé`.
        assert_eq!(parts("éAÉé").0, [1, 2]);
    }

    #[test]
    fn a_run_of_only_underscores_is_a_subword() {
        assert_eq!(parts("a ___ b"), (vec![], vec![]));
        assert_eq!(parts("_"), (vec![], vec![]));
        assert_eq!(parts("_.a_"), (vec![3], vec![3]));
    }

    #[test]
    fn nothing_splits_or_is_passed_on_an_empty_row_or_outside_keywords() {
        assert_eq!(parts(""), (vec![], vec![]));
        assert_eq!(parts("a.B-c B"), (vec![], vec![]));
        // A digit beside punctuation, a letter beside an emoji.
        assert_eq!(parts("1.a😀b"), (vec![], vec![]));
    }

    #[test]
    fn a_measured_run_is_reused_and_others_are_measured_afresh() {
        let text = Text::new("a___b ___");
        let cursor = Cursor::at(&text, 0);
        assert!(passed_over(&cursor, 2));
        assert_eq!(cursor.underscores.get(), Some((1, 4, true)));
        assert!(passed_over(&cursor, 3));
        assert!(!passed_over(&cursor, 7));
        assert_eq!(cursor.underscores.get(), Some((6, 9, false)));
    }
}
