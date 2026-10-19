//! fe-vim's insert-mode keyword completion (`<C-n>` and `<C-p>`) against
//! recorded Neovim: `tests/corpus/insert_completion.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.
//! Also what [`VimState::completion`] gives a host, and searches through
//! large buffers, bounded by the time limit. The limit is process-wide, so
//! every test here holds `LIMIT`.

#[allow(dead_code)]
mod support;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fe_buffer::ResolvedSelection;
use fe_vim::search::{self, DEFAULT_TIME_LIMIT, set_time_limit};
use fe_vim::{FeedError, HeadlessVim, KeywordCompletion, Splits, VimKey};
use support::{check, load};

static LIMIT: Mutex<()> = Mutex::new(());

/// Holds `LIMIT` with the time limit `limit`, restoring the default on drop.
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

#[test]
fn insert_completion_matches_neovim() {
    let _limit = limited(DEFAULT_TIME_LIMIT);
    let recorded = load("insert_completion").unwrap();
    assert!(
        recorded.len() >= 16,
        "insert_completion: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_n_first_match",
        "ctrl_n_cycles",
        "ctrl_n_wraps_to_typed",
        "ctrl_p_backward",
        "ctrl_p_cycles",
        "ctrl_n_no_match_fails",
        "ctrl_n_no_prefix",
        "ctrl_e_restores",
        "ctrl_y_accepts",
        "typing_accepts",
        "ctrl_n_dedupes",
        "ctrl_n_unicode_word",
        "ctrl_n_dot_repeat",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "insert_completion: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "insert_completion: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// What a host shows: the candidates listed so far in nvim's menu order (as
/// found for `<C-n>`, the other way round for `<C-p>`), the selected one as
/// the keys move through them, and where the typed keyword is; nothing once
/// completion ends.
#[test]
fn completion_state_follows_cycling() {
    let _limit = limited(DEFAULT_TIME_LIMIT);
    let words = |words: &[&str]| {
        words
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>()
    };
    // The typed `f` on row 1, after "foo food fool\n".
    let typed = 14..15;
    let mut vim = HeadlessVim::new("foo food fool\n\n-", (1, 0));
    vim.feed("if").unwrap();
    assert_eq!(vim.state().completion(vim.model()), None);
    let expect = |vim: &mut HeadlessVim, keys: &str, listed: &[&str], selected, all| {
        vim.feed(keys).unwrap();
        assert_eq!(
            vim.state().completion(vim.model()),
            Some(KeywordCompletion {
                candidates: words(listed),
                selected,
                typed: typed.clone(),
                listed_all: all,
            }),
            "after {keys}"
        );
    };
    expect(&mut vim, "<C-n>", &["foo"], Some(0), false);
    expect(&mut vim, "<C-n>", &["foo", "food"], Some(1), false);
    expect(&mut vim, "<C-n>", &["foo", "food", "fool"], Some(2), false);
    // Past the last, the typed keyword: the search has read every row.
    expect(&mut vim, "<C-n>", &["foo", "food", "fool"], None, true);
    expect(&mut vim, "<C-p>", &["foo", "food", "fool"], Some(2), true);
    assert_eq!(vim.text(), "foo food fool\nfool\n-");
    vim.feed("x").unwrap();
    assert_eq!(vim.state().completion(vim.model()), None);
    assert_eq!(vim.text(), "foo food fool\nfoolx\n-");

    let mut vim = HeadlessVim::new("foo food fool\n\n-", (1, 0));
    vim.feed("if").unwrap();
    expect(&mut vim, "<C-p>", &["fool"], Some(0), false);
    expect(&mut vim, "<C-p>", &["food", "fool"], Some(0), false);
    expect(&mut vim, "<C-n>", &["food", "fool"], Some(1), false);
    // Back from the typed keyword to the first in the menu: all found.
    expect(
        &mut vim,
        "<C-n><C-n>",
        &["foo", "food", "fool"],
        Some(0),
        true,
    );
    vim.feed("<C-e>").unwrap();
    assert_eq!(vim.state().completion(vim.model()), None);
    assert_eq!(vim.text(), "foo food fool\nf\n-");

    // `<C-y>` and `<Esc>` end it too, and with no match it never starts.
    vim.feed("<C-n><C-y>").unwrap();
    assert_eq!(vim.state().completion(vim.model()), None);
    vim.feed("<C-n>").unwrap();
    assert!(vim.state().completion(vim.model()).is_some());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.state().completion(vim.model()), None);
    vim.feed("ozz<C-n>").unwrap();
    assert_eq!(vim.state().completion(vim.model()), None);
    assert_eq!(vim.state().message(), Some("Pattern not found"));
}

/// A `<C-n>` that finds nothing in 200,000 rows reads them all within the
/// command deadline ([`search::time_limit`]) and changes nothing.
#[test]
fn insert_completion_large_buffer() {
    let _limit = limited(DEFAULT_TIME_LIMIT);
    let rows = 200_000;
    let text: String = (0..rows)
        .map(|row| format!("alpha beta gamma delta {row}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut vim = HeadlessVim::new(&text, (rows / 2, 0));
    vim.feed("Azq").unwrap();
    let typed = vim.text();
    let started = Instant::now();
    vim.feed("<C-n>").unwrap();
    let took = started.elapsed();
    assert!(
        took < search::time_limit(),
        "<C-n> took {took:?}, past the {:?} deadline",
        search::time_limit()
    );
    assert_eq!(vim.state().message(), Some("Pattern not found"));
    assert_eq!(vim.state().completion(vim.model()), None);
    assert!(vim.text() == typed, "the text changed");
    assert_eq!(
        vim.cursor(),
        (rows / 2, "alpha beta gamma delta 100000zq".len())
    );
}

/// One row of 300,000 copies of the same word: once it's found, the next
/// `<C-n>` looks at each copy, a step at a time, so with no time at all it
/// stops after a few, changing nothing, and given the time it reads them
/// all and comes back to the typed keyword.
#[test]
fn insert_completion_dense_row() {
    let dense = "foo ".repeat(300_000);
    let text = format!("{dense}\n");
    let mut vim = HeadlessVim::new(&text, (1, 0));
    {
        let _limit = limited(DEFAULT_TIME_LIMIT);
        vim.feed("ifo<C-n>").unwrap();
        assert_eq!(vim.text(), format!("{dense}\nfoo"));
    }
    {
        let _limit = limited(Duration::ZERO);
        let started = Instant::now();
        vim.feed("<C-n>").unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(vim.state().message(), Some("Interrupted"));
        assert_eq!(vim.text(), format!("{dense}\nfoo"));
        let completion = vim.state().completion(vim.model()).unwrap();
        assert_eq!(
            (completion.selected, completion.listed_all),
            (Some(0), false)
        );
    }
    // Loaded debug builds can take a while over the copies.
    let _limit = limited(Duration::from_secs(120));
    vim.feed("<C-n>").unwrap();
    assert_eq!(vim.state().message(), None);
    assert_eq!(vim.text(), format!("{dense}\nfo"));
    let completion = vim.state().completion(vim.model()).unwrap();
    assert_eq!(completion.candidates, ["foo"]);
    assert_eq!((completion.selected, completion.listed_all), (None, true));
}

/// With no time at all, each `<C-n>` reads a few windows of rows and
/// stops, changing nothing; the next goes on from there, so they reach a
/// match far away.
#[test]
fn interrupted_completion_goes_on() {
    let _limit = limited(Duration::ZERO);
    let rows = 100_000;
    let mut text: Vec<String> = (0..rows).map(|_| "x y z".to_owned()).collect();
    text.push("far".to_owned());
    let mut vim = HeadlessVim::new(&text.join("\n"), (0, 0));
    vim.feed("ifa").unwrap();
    let typed = vim.text();
    let mut keys = 0;
    while vim.text() == typed {
        assert!(keys < rows, "no match after {keys} keys");
        vim.feed("<C-n>").unwrap();
        keys += 1;
        if vim.text() == typed {
            assert_eq!(vim.state().message(), Some("Interrupted"));
            let completion = vim.state().completion(vim.model()).unwrap();
            assert_eq!(completion.candidates, Vec::<String>::new());
        }
    }
    assert!(keys > 1, "found in one key");
    assert!(vim.text().starts_with("farx y z"));
    let completion = vim.state().completion(vim.model()).unwrap();
    assert_eq!(completion.candidates, ["far"]);
}

/// With a caret at each row, the keyword typed at both and completed at
/// both: the range is where the primary caret's keyword is now, after the
/// text completion put in at the caret before it.
#[test]
fn completion_typed_range_follows_carets_before() {
    let _limit = limited(DEFAULT_TIME_LIMIT);
    let mut vim = HeadlessVim::new("foo food\n\n", (1, 0));
    let model = vim.model_mut();
    model.set_selections([9, 10].map(|at| ResolvedSelection {
        range: at..at,
        reversed: false,
    }));
    vim.feed("if").unwrap();
    assert_eq!(vim.text(), "foo food\nf\nf");
    for (keys, text) in [
        ("<C-n>", "foo food\nfoo\nfoo"),
        ("<C-n>", "foo food\nfood\nfood"),
        ("<C-p>", "foo food\nfoo\nfoo"),
    ] {
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), text);
        let completion = vim.state().completion(vim.model()).unwrap();
        let primary = vim.model().primary_selection().head();
        let word = completion.candidates[completion.selected.unwrap()].clone();
        let start = primary - word.len();
        assert_eq!(completion.typed, start..start + 1, "after {keys}");
        assert_eq!(&vim.text()[completion.typed.clone()], "f");
    }
    vim.feed("<C-e>").unwrap();
    assert_eq!(vim.text(), "foo food\nf\nf");
}

/// Whatever ends the session or changes the text or the carets ends the
/// completion as `completion` sees it: a split, an edit that's not vim's,
/// a caret moved.
#[test]
fn completion_ends_with_the_session_or_a_change() {
    let _limit = limited(DEFAULT_TIME_LIMIT);
    let start = |keys: &str| {
        let mut vim = HeadlessVim::new("foo food fool\n\n-", (1, 0));
        vim.feed(keys).unwrap();
        assert!(vim.state().completion(vim.model()).is_some());
        vim
    };
    let mut vim = start("if<C-n>");
    vim.split_session(Splits::Moves, |model| model.set_cursor(0));
    assert_eq!(vim.state().completion(vim.model()), None);
    let mut vim = start("if<C-n>");
    vim.split_session(Splits::Other, |_| ());
    assert_eq!(vim.state().completion(vim.model()), None);
    // The next `<C-n>` starts again, from the keyword now before the caret.
    vim.feed("<C-n>").unwrap();
    assert_eq!(vim.text(), "foo food fool\nfoo\n-");
    let mut vim = start("if<C-n>");
    vim.model_mut().insert("z");
    assert_eq!(vim.state().completion(vim.model()), None);
    let mut vim = start("if<C-n>");
    vim.model_mut().set_cursor(0);
    assert_eq!(vim.state().completion(vim.model()), None);
}

/// Presses `<C-n>` until the text changes or `done` says so, with no time
/// at all: each key that stops short changes nothing and says
/// `Interrupted`. Returns how many keys it took.
fn press_until(vim: &mut HeadlessVim, done: impl Fn(&HeadlessVim) -> bool) -> usize {
    let before = vim.text();
    let mut keys = 0;
    loop {
        assert!(keys < 10_000, "still going after {keys} keys");
        vim.feed("<C-n>").unwrap();
        keys += 1;
        if vim.text() != before || done(vim) {
            return keys;
        }
        assert_eq!(vim.state().message(), Some("Interrupted"));
    }
}

/// One word of 3,000,000 bytes: measuring it is many steps, so with no
/// time at all the keys stop part way, changing nothing, and later ones go
/// on until it's taken whole.
#[test]
fn completion_long_word_goes_on() {
    let _limit = limited(Duration::ZERO);
    let word = format!("f{}", "o".repeat(3_000_000));
    let mut vim = HeadlessVim::new(&format!("{word}\n"), (1, 0));
    vim.feed("if").unwrap();
    let keys = press_until(&mut vim, |_| false);
    assert!(keys > 1, "taken in one key");
    assert_eq!(vim.text(), format!("{word}\n{word}"));
    let completion = vim.state().completion(vim.model()).unwrap();
    assert_eq!(completion.candidates, [word]);
}

/// One row of 3,000,000 bytes with no match: read a window at a time, so
/// with no time at all the keys stop part way, and later ones go on until
/// it's all read and nothing matched.
#[test]
fn completion_long_row_without_match_goes_on() {
    let _limit = limited(Duration::ZERO);
    let row = "x".repeat(3_000_000);
    let mut vim = HeadlessVim::new(&format!("{row}\n"), (1, 0));
    vim.feed("izq").unwrap();
    let keys = press_until(&mut vim, |vim| {
        vim.state().message() == Some("Pattern not found")
    });
    assert!(keys > 1, "read in one key");
    assert_eq!(vim.text(), format!("{row}\nzq"));
    assert_eq!(vim.state().completion(vim.model()), None);
}

/// A selection changed with its head kept, as selecting the word just
/// completed does, ends the completion too: `completion` sees none, and
/// `<C-y>` isn't completion's.
#[test]
fn completion_ends_with_a_selection_kept_at_its_head() {
    let _limit = limited(DEFAULT_TIME_LIMIT);
    let mut vim = HeadlessVim::new("foo food fool\n\n-", (1, 0));
    vim.feed("if<C-n>").unwrap();
    assert_eq!(vim.text(), "foo food fool\nfoo\n-");
    vim.model_mut().select_range(14..17, false);
    assert_eq!(vim.model().primary_selection().head(), 17);
    assert_eq!(vim.state().completion(vim.model()), None);
    assert_eq!(
        vim.feed("<C-y>"),
        Err(FeedError::Forwarded(VimKey::Ctrl('y')))
    );
    assert_eq!(vim.text(), "foo food fool\nfoo\n-");
}
