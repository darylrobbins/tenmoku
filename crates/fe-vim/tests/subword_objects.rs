//! `iS aS`: the subword under the caret, always on (nvim-various-textobjs'
//! subword object); `set_subword_motions` only changes `w b e ge`.

use fe_vim::HeadlessVim;

fn run(text: &str, col: usize, keys: &str, motions: bool) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, col));
    vim.state_mut().set_subword_motions(motions);
    let _ = vim.feed(keys);
    vim
}

fn after(text: &str, col: usize, keys: &str) -> String {
    run(text, col, keys, false).text()
}

/// What `keys` (an object after `y` or `v`) covers.
fn yanked(text: &str, col: usize, keys: &str) -> String {
    let vim = run(text, col, &format!("{keys}y"), false);
    vim.registers()
        .get('"')
        .map(|r| r.text.clone())
        .unwrap_or_default()
}

#[test]
fn subword_objects_select() {
    assert_eq!(after("fooBarBaz", 4, "diS"), "fooBaz");
    assert_eq!(after("fooBarBaz", 4, "ciSX<Esc>"), "fooXBaz");
    assert_eq!(yanked("fooBarBaz", 7, "yiS"), "Baz");
    assert_eq!(after("fooBarBaz", 0, "daS"), "BarBaz");
    assert_eq!(after("HTTPServer", 1, "diS"), "Server");
    assert_eq!(after("HTTPServer", 6, "diS"), "HTTP");
    assert_eq!(after("snake_case_name", 1, "diS"), "_case_name");
    assert_eq!(after("snake_case_name", 1, "daS"), "case_name");
    assert_eq!(after("snake_case_name", 7, "daS"), "snake_name");
    assert_eq!(after("snake_case_name", 11, "daS"), "snake_case");
    // On a `_` run inside a keyword run: that run.
    assert_eq!(after("snake_case", 5, "diS"), "snakecase");
    assert_eq!(after("a fooBar b", 3, "daS"), "a Bar b");
    assert_eq!(after("a fooBar b", 6, "daS"), "a foob");
    assert_eq!(after("a fooBar b", 5, "diS"), "a foo b");
    // Blanks as `iw`.
    assert_eq!(after("a  b", 2, "diS"), "ab");
    // Counts.
    assert_eq!(after("fooBarBazQux", 0, "d2iS"), "BazQux");
    assert_eq!(after("fooBarBazQux", 3, "d2aS"), "fooQux");
    assert_eq!(after("foo_bar_baz", 0, "d2iS"), "bar_baz");
    assert_eq!(after("foo_bar_baz", 0, "d2aS"), "baz");
}

#[test]
fn subword_objects_in_visual() {
    assert_eq!(yanked("fooBarBaz", 4, "viS"), "Bar");
    assert_eq!(yanked("fooBarBaz", 4, "viSiS"), "BarBaz");
    assert_eq!(yanked("fooBarBazQux", 0, "viSiSiS"), "fooBarBaz");
    assert_eq!(yanked("a fooBar b", 3, "vaS"), "foo");
    assert_eq!(yanked("snake_case_name", 1, "vaS"), "snake_");
    assert_eq!(yanked("snake_case_name", 1, "vaSaS"), "snake_case_");
    assert_eq!(yanked("fooBarBazQux", 0, "v2iS"), "fooBar");
}

#[test]
fn subword_objects_need_no_setting() {
    for motions in [false, true] {
        let vim = run("fooBarBaz", 4, "ciSX<Esc>", motions);
        assert_eq!(vim.text(), "fooXBaz");
        let vim = run("fooBarBaz", 4, "daS", motions);
        assert_eq!(vim.text(), "fooBaz");
    }
    // `.` repeats at the caret.
    let mut vim = run("fooBarBaz", 4, "ciSX<Esc>", false);
    let _ = vim.feed("l.");
    assert_eq!(vim.text(), "fooXX");
}

#[test]
fn a_subword_takes_the_underscores_before_it_when_none_follow() {
    assert_eq!(after("foo_barBaz", 5, "daS"), "fooBaz");
    assert_eq!(after("__init", 3, "daS"), "");
    assert_eq!(yanked("foo_barBaz", 5, "vaS"), "_bar");
    assert_eq!(yanked("__init", 3, "vaS"), "__init");
}

#[test]
fn underscore_runs_are_apart_from_blanks() {
    assert_eq!(after("foo__ bar", 3, "diS"), "foo bar");
    assert_eq!(after("foo__ bar", 5, "diS"), "foo__bar");
    assert_eq!(after("foo__ bar", 3, "d2iS"), "foobar");
    assert_eq!(yanked("foo__ bar", 3, "viS"), "__");
    assert_eq!(yanked("foo__ bar", 3, "viSiS"), "__ ");
    assert_eq!(yanked("foo __bar", 4, "viS"), "__");
    assert_eq!(yanked("foo __bar", 4, "viSiS"), "__bar");
}

#[test]
fn one_char_subwords_grow_in_visual() {
    assert_eq!(yanked("utf8Decode", 3, "viSiS"), "8Decode");
    assert_eq!(yanked("utf8Decode", 3, "viSiSiS"), "8Decode");
    assert_eq!(yanked("aBcD", 0, "viSiS"), "aBc");
    assert_eq!(yanked("utf8Decode", 3, "vaSaS"), "8Decode");
}
