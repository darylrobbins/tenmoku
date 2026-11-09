//! Find-bar queries (`Pattern::from_query`, `Pattern::find_from`).

// One match is a list of one range here, not a range to collect.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use fe_text::Text;

use crate::search::{FindQuery, Pattern, PatternError, ROWS_SEARCHED, matches_in};

fn query(text: &str, regex: bool, match_case: bool, whole_word: bool) -> FindQuery {
    FindQuery {
        text: text.into(),
        regex,
        match_case,
        whole_word,
    }
}

fn all(query: &FindQuery, text: &str) -> Vec<Range<usize>> {
    let pattern = Pattern::from_query(query).unwrap();
    matches_in(&Text::new(text), 0..u32::MAX, &pattern, usize::MAX)
}

#[test]
fn literal_regex_and_case() {
    assert_eq!(all(&query("a.b", false, false, false), "a.b axb"), [0..3]);
    assert_eq!(
        all(&query("a.b", true, false, false), "a.b axb"),
        [0..3, 4..7]
    );
    assert_eq!(
        all(&query("foo", false, false, false), "foo FOO"),
        [0..3, 4..7]
    );
    assert_eq!(all(&query("foo", false, true, false), "foo FOO"), [0..3]);
    assert_eq!(
        Pattern::from_query(&FindQuery::default()).unwrap_err(),
        PatternError::Invalid("invalid pattern: empty pattern".into())
    );
    assert!(matches!(
        Pattern::from_query(&query("(", true, false, false)),
        Err(PatternError::Regex(message)) if !message.contains('\n')
    ));
    assert!(all(&query("a\\nb", true, false, false), "a\nb").is_empty());
}

#[test]
fn whole_word_like_project_search() {
    let foo = query("foo", false, false, true);
    assert_eq!(all(&foo, "foo.bar"), [0..3]);
    assert_eq!(all(&foo, "(foo)"), [1..4]);
    assert_eq!(all(&foo, "foofoo foo"), [7..10]);
    for text in ["foobar", "_foo", "éfoo"] {
        assert!(all(&foo, text).is_empty(), "{text}");
    }
}

#[test]
fn find_from_includes_from() {
    let pattern = Pattern::from_query(&query("ab", false, false, false)).unwrap();
    let text = Text::new("ab x\nab\nzz ab");
    assert_eq!(pattern.find_from(&text, 0), Some(0..2));
    assert_eq!(pattern.find_from(&text, 1), Some(5..7));
    assert_eq!(pattern.find_from(&text, 5), Some(5..7));
    assert_eq!(pattern.find_from(&text, 6), Some(11..13));
    // Wraps once, to a match before `from`.
    assert_eq!(pattern.find_from(&text, 12), Some(0..2));
    let none = Pattern::from_query(&query("q", false, false, false)).unwrap();
    ROWS_SEARCHED.with(|rows| rows.set(0));
    assert_eq!(none.find_from(&text, 6), None);
    assert_eq!(ROWS_SEARCHED.with(|rows| rows.get()), 3);
}

#[test]
fn find_from_overlaps_and_row_end() {
    let aa = Pattern::from_query(&query("aa", false, false, false)).unwrap();
    assert_eq!(aa.find_from(&Text::new("aaa"), 1), Some(1..3));
    let end = Pattern::from_query(&query("a|$", true, false, false)).unwrap();
    assert_eq!(end.find_from(&Text::new("a"), 1), Some(1..1));
}

#[test]
fn whole_word_steps_by_character() {
    let dot = query(".", true, false, true);
    assert_eq!(all(&dot, " \u{0301} "), [1..3]);
}

#[test]
fn whole_word_resumes_at_row_end() {
    let end = Pattern::from_query(&query(".|$", true, false, true)).unwrap();
    assert_eq!(end.find_from(&Text::new("a "), 1), Some(2..2));
}
