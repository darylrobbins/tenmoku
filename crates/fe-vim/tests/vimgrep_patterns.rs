//! `:vimgrep`'s pattern split at the delimiter, where a POSIX class's own
//! `]` doesn't end the collection around it.

use fe_vim::QuickfixAction;

#[test]
fn a_posix_class_bracket_stays_in_the_pattern() {
    let (pattern, flags, globs) = QuickfixAction::parse_vimgrep("/[[:alpha:]/]/g x").unwrap();
    assert_eq!(pattern, "[[:alpha:]/]");
    assert_eq!(flags, "g");
    assert_eq!(globs, ["x"]);
}

#[test]
fn a_class_with_a_delimiter_after_it_or_a_negation() {
    let split = |args: &str| QuickfixAction::parse_vimgrep(args).unwrap().0;
    assert_eq!(split("/[^[:alpha:]/x]/"), "[^[:alpha:]/x]");
    assert_eq!(split("/[[:alpha:][:digit:]/]/ f"), "[[:alpha:][:digit:]/]");
    assert_eq!(split("/[[:alpha:]]/ f"), "[[:alpha:]]");
}
