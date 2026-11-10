//! T564 regressions of the tag objects, through keys only: each fails on the
//! code before the change. Neovim 0.12.4's answers are recorded in
//! `tests/corpus/delimiters.toml`.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use fe_vim::HeadlessVim;
use fe_vim::search::{DEFAULT_TIME_LIMIT, set_time_limit};

static LIMIT: Mutex<()> = Mutex::new(());

/// Holds the process-wide search limit for one test; puts the default back
/// when dropped.
struct Limit(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Limit {
    fn set(limit: Duration) -> Self {
        let guard = LIMIT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        set_time_limit(limit);
        Self(guard)
    }
}

impl Drop for Limit {
    fn drop(&mut self) {
        set_time_limit(DEFAULT_TIME_LIMIT);
    }
}

/// Keeps a search from giving up on a loaded machine's clock.
fn unhurried() -> Limit {
    Limit::set(Duration::from_secs(60))
}

/// The text and unnamed register after `keys` with the caret at `(0, column)`.
fn run(text: &str, column: usize, keys: &str) -> (String, String) {
    let mut vim = HeadlessVim::new(text, (0, column));
    vim.feed(keys).unwrap();
    let register = vim
        .registers()
        .get('"')
        .map(|register| register.text.clone())
        .unwrap_or_default();
    (vim.text(), register)
}

fn unchanged(text: &str, column: usize, keys: &str) {
    assert_eq!(run(text, column, keys), (text.to_string(), String::new()));
}

#[test]
fn distinct_joined_chars_in_a_pattern_name_keep_their_own_requirements() {
    let _limit = unhurried();
    unchanged(
        "<a\u{200d}a\u{200d}\u{301}.>x</a\u{200d}a\u{200d}x>",
        0,
        "dit",
    );
}

#[test]
fn a_magic_dot_before_a_zwj_pairs_with_nothing() {
    let _limit = unhurried();
    unchanged("<a.\u{200d}>x</a.\u{200d}>", 0, "dit");
}

#[test]
fn a_star_before_a_zwj_is_a_multi() {
    let _limit = unhurried();
    let (text, register) = run("<a*\u{200d}>x</aaa\u{200d}>", 0, "dit");
    assert_eq!(
        (text.as_str(), register.as_str()),
        ("<a*\u{200d}></aaa\u{200d}>", "x")
    );
}

#[test]
fn a_private_use_char_in_the_text_is_not_a_stand_in() {
    let _limit = unhurried();
    unchanged("<a\u{200d}.>x</\u{f0000}x>", 0, "dit");
}

#[test]
fn a_bare_lt_before_an_empty_closing_tag_keeps_the_boundary() {
    let _limit = unhurried();
    let (text, register) = run("<ab c><</>x</ab>", 6, "dit");
    assert_eq!((text.as_str(), register.as_str()), ("<ab c></ab>", "<</>x"));
}

#[test]
fn a_keyword_atom_in_a_pattern_name_pairs() {
    let _limit = unhurried();
    // The first `\k` works the keyword table out (slow unoptimized, and the
    // command clock counts it): do that outside the cases, which run on a
    // loaded machine in the gate. The result of this run doesn't matter.
    let _ = run("<\\ka>x</ba>", 0, "dit");
    for (text, left) in [
        ("<\\ka>x</ba>", "<\\ka></ba>"),
        ("<\\k*>x</abc>", "<\\k*></abc>"),
    ] {
        let (got, register) = run(text, 0, "dit");
        assert_eq!((got.as_str(), register.as_str()), (left, "x"), "{text}");
    }
}

#[test]
fn pattern_pairing_past_many_other_tags_finishes_in_time() {
    // This one is about the default limit: no test that raises it runs
    // alongside.
    let _limit = Limit::set(DEFAULT_TIME_LIMIT);
    // 20,000 `<b>` can't start a tag named `a.b`; trying each against every
    // span after it runs past the search time limit, which fails the object.
    let many = "<b>".repeat(20_000);
    let text = format!("<a.b>{many}x</axb>");
    let (got, register) = run(&text, 0, "dit");
    assert_eq!(register, format!("{many}x"));
    assert_eq!(got, "<a.b></axb>");
}

#[test]
fn other_character_class_atoms_in_a_pattern_name_pair() {
    let _limit = unhurried();
    for (text, left) in [
        ("<\\ia>x</ba>", "<\\ia></ba>"),
        ("<\\fa>x</.a>", "<\\fa></.a>"),
        ("<\\pa>x</ba>", "<\\pa></ba>"),
        ("<\\Ia>x</ba>", "<\\Ia></ba>"),
    ] {
        let (got, register) = run(text, 0, "dit");
        assert_eq!((got.as_str(), register.as_str()), (left, "x"), "{text}");
    }
    unchanged("<\\Ia>x</1a>", 0, "dit");
    unchanged("<\\ia>x</-a>", 0, "dit");
}

#[test]
fn eleven_distinct_joined_chars_in_a_pattern_name_pair() {
    let _limit = unhurried();
    let joined: String = "abcdefghijk"
        .chars()
        .map(|ch| format!("{ch}\u{200d}"))
        .collect();
    let text = format!("<{joined}.>x</{joined}z>");
    let (got, register) = run(&text, 0, "dit");
    assert_eq!(register, "x");
    assert_eq!(got, format!("<{joined}.></{joined}z>"));
}

#[test]
fn a_match_boundary_atom_in_a_pattern_name_is_not_prefiltered() {
    let _limit = unhurried();
    // `\zs` and `\ze` move where a match starts or ends from its `<`, so the
    // row search can't say where a tag's `<` is: each span is tried.
    for text in ["<a\\zsb>x</ab>", "<a\\zeb>x</ab>", "<ab\\zs>x</ab>"] {
        let (got, register) = run(text, 0, "dit");
        assert_eq!(
            (got.as_str(), register.as_str()),
            (text.replace("x", "").as_str(), "x"),
            "{text}"
        );
    }
}
