//! The find bar's replacements ([`fe_vim::search::replace`]): templates,
//! `replace_all`, and agreement with `:s` for equivalent patterns. The time
//! limit is process-wide, so tests that read or set it hold `LIMIT`.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use fe_text::Text;
use fe_vim::HeadlessVim;
use fe_vim::search::replace::{Replacement, replace_all};
use fe_vim::search::{
    DEFAULT_TIME_LIMIT, FindQuery, Interrupted, Pattern, matches_in, set_time_limit,
};

static LIMIT: Mutex<()> = Mutex::new(());

/// Holds `LIMIT` and restores the default limit on drop.
struct Limited(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Limited {
    fn drop(&mut self) {
        set_time_limit(DEFAULT_TIME_LIMIT);
    }
}

fn limited(limit: Duration) -> Limited {
    let guard = LIMIT.lock().unwrap_or_else(|p| p.into_inner());
    set_time_limit(limit);
    Limited(guard)
}

fn query(text: &str, regex: bool, match_case: bool, whole_word: bool) -> Pattern {
    Pattern::from_query(&FindQuery {
        text: text.into(),
        regex,
        match_case,
        whole_word,
    })
    .unwrap()
}

/// `text` with `edits` applied.
fn applied(text: &str, edits: &[(std::ops::Range<usize>, String)]) -> String {
    let mut out = text.to_string();
    for (range, with) in edits.iter().rev() {
        out.replace_range(range.clone(), with);
    }
    out
}

fn expand(pattern: &Pattern, text: &str, template: &str, regex: bool) -> Option<String> {
    let text = Text::new(text);
    let with = Replacement::parse(template, regex);
    let range = matches_in(&text, 0..1, pattern, 1).into_iter().next()?;
    pattern.replacement_at(&text, range, &with)
}

#[test]
fn template_expansion_with_regex_on() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    let p = query(r"(?P<w>\w+)-(\w+)", true, true, false);
    let go = |t: &str| expand(&p, "ab-cd", t, true).unwrap();
    assert_eq!(go("$0"), "ab-cd");
    assert_eq!(go("$&"), "ab-cd");
    assert_eq!(go("$2-$1"), "cd-ab");
    assert_eq!(go("$1a"), "aba");
    assert_eq!(go("${w}x"), "abx");
    assert_eq!(go("${nope}|$9|$99"), "||");
    assert_eq!(go("$$1"), "$1");
    assert_eq!(go(r"a\nb\tc\\d\q"), "a\nb\tc\\d\\q");
    assert_eq!(go("$ ${"), "$ ${");
}

#[test]
fn template_is_literal_with_regex_off() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    let p = query("foo", false, true, false);
    let t = r"$0 $1 ${x} $$ \n \t \\";
    assert_eq!(expand(&p, "foo", t, false).unwrap(), t);
}

#[test]
fn replace_all_matches_matches_in() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    let src = "Foo foo\nfoobar foo\n\nxfoo";
    let text = Text::new(src);
    let with = Replacement::parse("X", false);
    for (case, whole) in [(true, false), (false, false), (true, true), (false, true)] {
        let p = query("foo", false, case, whole);
        let edits = replace_all(&text, &p, &with).unwrap();
        let ranges: Vec<_> = edits.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(
            ranges,
            matches_in(&text, 0..4, &p, usize::MAX),
            "{case} {whole}"
        );
        for (range, _) in &edits {
            assert!(p.replacement_at(&text, range.clone(), &with).is_some());
        }
    }
    let p = query("foo", false, true, true);
    assert_eq!(replace_all(&text, &p, &with).unwrap().len(), 2);
    // Not a match: a part of one, and a range over two rows.
    assert_eq!(p.replacement_at(&text, 1..3, &with), None);
    assert_eq!(p.replacement_at(&text, 0..src.len(), &with), None);
    assert_eq!(p.replacement_at(&text, 0..3, &with), None, "Foo, case on");
}

#[test]
fn empty_matches_once() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    let text = Text::new("ab\n\nc");
    let p = query("x*", true, true, false);
    let with = Replacement::parse("-", true);
    let edits = replace_all(&text, &p, &with).unwrap();
    assert_eq!(applied("ab\n\nc", &edits), "-a-b\n-\n-c");
}

#[test]
fn replace_all_interrupted_never_partial() {
    let _guard = limited(Duration::from_millis(1));
    let src = "a".repeat(400_000);
    let text = Text::new(&src);
    let p = query(r"(a)", true, true, false);
    let with = Replacement::parse("$1b", true);
    let result = replace_all(&text, &p, &with);
    assert_eq!(result, Err(Interrupted));
}

#[test]
fn a_slow_final_scan_with_no_match_is_interrupted() {
    let _guard = limited(Duration::from_millis(1));
    let text = Text::new(&"a".repeat(20_000_000));
    let p = query("a+b", true, true, false);
    let with = Replacement::parse("X", true);
    assert_eq!(replace_all(&text, &p, &with), Err(Interrupted));
}

#[test]
fn literal_agrees_with_substitute() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    let src = "foo Foo\nfoofoo bar\n\nxfoo foo.";
    let edits = replace_all(
        &Text::new(src),
        &query("foo", false, true, false),
        &Replacement::parse("bar", false),
    )
    .unwrap();
    let mut vim = HeadlessVim::new(src, (0, 0));
    vim.feed(":%s/foo/bar/g<CR>").unwrap();
    assert_eq!(applied(src, &edits), vim.text());
}

#[test]
fn regex_groups_agree_with_substitute() {
    let _guard = limited(DEFAULT_TIME_LIMIT);
    let src = "ab-cd x-y\n-- e-u\none two-three";
    let edits = replace_all(
        &Text::new(src),
        &query(r"(\w+)-(\w+)", true, true, false),
        &Replacement::parse("$2-$1", true),
    )
    .unwrap();
    let mut vim = HeadlessVim::new(src, (0, 0));
    vim.feed(r":%s/\(\w\+\)-\(\w\+\)/\2-\1/g<CR>").unwrap();
    assert_eq!(applied(src, &edits), vim.text());
}
