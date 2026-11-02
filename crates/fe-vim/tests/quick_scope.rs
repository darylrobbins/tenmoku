//! Quick-scope targets and the direction of a pending find.

use fe_text::Text;
use fe_vim::quick_scope::{MAX_CHARS, Target, targets};
use fe_vim::{Pending, Step, VimKey, step};

fn at(row: &str, caret: usize, forward: bool) -> Vec<(usize, bool)> {
    targets(&Text::new(row), caret, forward)
        .into_iter()
        .map(|Target { offset, primary }| (offset, primary))
        .collect()
}

#[test]
fn quick_scope_targets() {
    // Forward: the first unique char of each later word.
    assert_eq!(at("foo bar baz", 0, true), [(4, true), (10, true)]);
    // Backward, in row order; each word's leftmost qualifying char.
    assert_eq!(at("foo bar baz", 10, false), [(0, true), (6, true)]);
    // `foo` counted: its o's are gone, `oo` has no target; `b` of bar is new.
    assert_eq!(at("foo oo bar", 0, true), [(7, true)]);
    // Secondary: the caret word's `ab` counted once, so the next `ab` is
    // seen twice; the word after that is seen three times and has none.
    assert_eq!(at("aab ab ab", 0, true), [(4, false)]);
    // Backward secondary: `ab` was counted once, the next `ab` twice.
    assert_eq!(at("ab ab baa", 8, false), [(3, false)]);
    // A caret on a blank: the adjacent word is not skipped, either way.
    assert_eq!(at("ab  cd", 2, true), [(4, true)]);
    assert_eq!(at("ab  cd", 3, false), [(0, true)]);
    // Case-sensitive.
    assert_eq!(at("a A", 0, true), [(2, true)]);
    // A caret on a blank skips nothing.
    assert_eq!(at("x  yz", 1, true), [(3, true)]);
    // Only the caret's row.
    assert_eq!(at("ab\ncd ef", 0, true), []);
    assert_eq!(at("", 0, true), []);
    assert_eq!(at("", 0, false), []);
    // Non-ASCII chars separate words, in byte offsets.
    assert_eq!(at("a é bc", 0, true), [(5, true)]);
    assert_eq!(at("a xe\u{301}y", 0, true), [(2, true), (6, true)]);
}

#[test]
fn quick_scope_is_bounded() {
    // `z` sits past MAX_CHARS on the row: unread, it can't take the target.
    let mut row = String::from("a b");
    row.push_str(&" ".repeat(MAX_CHARS));
    row.push('z');
    row.push_str(&"q".repeat(100_000));
    assert_eq!(at(&row, 0, true), [(2, true)]);
    // Within reach it is read.
    let near = format!("a {}z", " ".repeat(MAX_CHARS - 3));
    assert_eq!(at(&near, 0, true).len(), 1);
    // Backward likewise: nothing before the window matters.
    let row = format!("z{}ab {}", "q".repeat(100_000), "c");
    let caret = row.len();
    let all = at(&row, caret, false);
    assert!(all.iter().all(|&(offset, _)| offset >= caret - MAX_CHARS));
    let far = format!("z{}c", " ".repeat(2 * MAX_CHARS));
    assert_eq!(at(&far, far.len() - 1, false), []);
}

#[test]
fn a_grapheme_cut_at_the_limit_is_a_separator() {
    let row = format!("a{}e\u{301}", " ".repeat(MAX_CHARS - 1));
    // `e` is the MAX_CHARS-th char after the caret; its mark follows.
    assert_eq!(at(&row, 0, true), []);
    let plain = format!("a{}e", " ".repeat(MAX_CHARS - 1));
    assert_eq!(at(&plain, 0, true), [(MAX_CHARS, true)]);
}

#[test]
fn a_caret_on_a_marked_base_is_on_no_word() {
    // `x` with its mark is a separator, so `ab` before it is a new word.
    assert_eq!(at("abx\u{301}", 2, false), [(0, true)]);
    assert_eq!(at("abx", 2, false), []);
    // Forward: the marked base is no word, so the word after is whole.
    assert_eq!(at("x\u{301}ab", 0, true), [(3, true)]);
    // The row going on past the limit drops the last char, unknown whole.
    let long = format!("a{}eq", " ".repeat(MAX_CHARS - 1));
    assert_eq!(at(&long, 0, true), []);
}

fn pending(keys: &str) -> Pending {
    let mut pending = Pending::new();
    for ch in keys.chars() {
        pending = match step(pending, VimKey::Char(ch)) {
            Step::Wait(next) => next,
            other => panic!("{keys}: {other:?}"),
        };
    }
    pending
}

#[test]
fn find_direction_reports_pending_finds() {
    assert_eq!(pending("f").find_direction(), Some(true));
    assert_eq!(pending("t").find_direction(), Some(true));
    assert_eq!(pending("df").find_direction(), Some(true));
    assert_eq!(pending("2t").find_direction(), Some(true));
    assert_eq!(pending("F").find_direction(), Some(false));
    assert_eq!(pending("T").find_direction(), Some(false));
    assert_eq!(pending("dF").find_direction(), Some(false));
    assert_eq!(pending("").find_direction(), None);
    assert_eq!(pending("d").find_direction(), None);
    assert_eq!(pending("g").find_direction(), None);
    assert!(matches!(
        step(pending("f"), VimKey::Char('x')),
        Step::Commit(_)
    ));
}
