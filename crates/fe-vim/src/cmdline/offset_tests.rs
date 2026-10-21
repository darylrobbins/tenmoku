use super::parse;
use crate::{
    HeadlessVim, Motion, Search, SearchOffset, SearchPattern,
    search::{ROWS_SEARCHED, regex_calls},
};

#[test]
fn offsets_parse() {
    use SearchOffset::{End, Lines, Start};
    for (text, offset) in [
        ("", SearchOffset::None),
        ("e", End(0)),
        ("e+1", End(1)),
        ("e-1", End(-1)),
        ("e+", End(1)),
        ("e-", End(-1)),
        ("e2", End(2)),
        ("s", Start(0)),
        ("s+2", Start(2)),
        ("s-1", Start(-1)),
        ("b", Start(0)),
        ("b-1", Start(-1)),
        ("b+3", Start(3)),
        ("+2", Lines(2)),
        ("-3", Lines(-3)),
        ("+", Lines(1)),
        ("-", Lines(-1)),
        ("4", Lines(4)),
        ("+0", Lines(0)),
    ] {
        assert_eq!(parse(text), Some(offset), "{text:?}");
    }
    for text in [
        ";/bar",
        "e;/bar",
        "x",
        "e x",
        " e",
        "e+1x",
        "+-1",
        "s+-1",
        "E",
        "e++",
        "99999999999",
    ] {
        assert_eq!(parse(text), None, "{text:?}");
    }
}

/// A chained search or an offset fe-vim doesn't read fails as before,
/// changing nothing.
#[test]
fn chained_and_malformed_offsets_fail() {
    for (keys, offset) in [("/foo/;/bar<CR>", ";/bar"), ("/foo/e x<CR>", "e x")] {
        let mut vim = HeadlessVim::new("a foo bar", (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.cursor(), (0, 0), "{keys}");
        assert_eq!(
            vim.state().message(),
            Some(format!("search offsets not supported: {offset}").as_str())
        );
        assert_eq!(vim.registers().last_search(), None, "{keys}");
    }
}

#[test]
fn a_search_serializes_its_offset_only_if_it_has_one() {
    let search = |offset| {
        Motion::Search(Search {
            pattern: SearchPattern::new("foo"),
            forward: true,
            offset,
            again: false,
            word: false,
            ignore_case: false,
        })
    };
    let plain = serde_json::json!({"search": {"pattern": "foo", "forward": true}});
    assert_eq!(
        serde_json::to_value(search(SearchOffset::None)).unwrap(),
        plain
    );
    assert_eq!(
        serde_json::from_value::<Motion>(plain).unwrap(),
        search(SearchOffset::None)
    );
    let end =
        serde_json::json!({"search": {"pattern": "foo", "forward": true, "offset": {"end": 1}}});
    assert_eq!(
        serde_json::to_value(search(SearchOffset::End(1))).unwrap(),
        end
    );
    assert_eq!(
        serde_json::from_value::<Motion>(end).unwrap(),
        search(SearchOffset::End(1))
    );
}

/// The last search keeps its offset for `/<CR>`, `n` and `N`; `//` drops it,
/// `//e` sets a new one, and `*` and an ex address's pattern drop it.
#[test]
fn the_last_search_keeps_its_offset() {
    let mut vim = HeadlessVim::new("a foo b foo c foo\nfoo", (0, 0));
    vim.feed("/foo/e<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 4));
    assert_eq!(vim.registers().last_search_offset(), SearchOffset::End(0));
    vim.feed("/<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 10));
    vim.feed("//<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 14));
    assert_eq!(vim.registers().last_search_offset(), SearchOffset::None);
    vim.feed("0//s+1<CR>n").unwrap();
    assert_eq!(vim.cursor(), (0, 9));
    vim.feed("*").unwrap();
    assert_eq!(vim.registers().last_search_offset(), SearchOffset::None);

    let mut vim = HeadlessVim::new("foo\nbar foo foo\nfoo", (0, 0));
    vim.feed("/foo/e<CR>:/foo/<CR>").unwrap();
    assert_eq!(vim.registers().last_search_offset(), SearchOffset::None);
    vim.feed("0n").unwrap();
    assert_eq!(vim.cursor(), (1, 4));
}

/// Counted `e` searches list a row's matches once, however many of them
/// the count steps over.
#[test]
fn counted_end_searches_read_each_row_once() {
    let row = "ab ".repeat(20_000);
    for (keys, from, to) in [
        ("5000/ab/e<CR>", (0, 0), (0, 14_998)),
        ("5000?ab?e<CR>", (0, 59_999), (0, 45_001)),
    ] {
        let mut vim = HeadlessVim::new(&row, from);
        ROWS_SEARCHED.set(0);
        vim.feed(keys).unwrap();
        assert_eq!(vim.cursor(), to, "{keys}");
        assert!(
            ROWS_SEARCHED.get() <= 3,
            "{keys}: {} rows",
            ROWS_SEARCHED.get()
        );
    }
}

/// A count of `n` going back over an empty match that finds itself again
/// (`?\>?e1` stays put) is counted round, not searched.
#[test]
fn a_counted_n_stuck_on_an_empty_match_is_quick() {
    let mut vim = HeadlessVim::new("ab ab ab", (0, 3));
    vim.feed("?\\>?e1<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    ROWS_SEARCHED.set(0);
    vim.feed("999999n").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    assert!(ROWS_SEARCHED.get() <= 4, "{} rows", ROWS_SEARCHED.get());
}

/// A count of searches for a pattern over rows steps along a row of dense
/// matches listing each once, going on from where the last step stopped,
/// not from the row's start, and reading the row once: regex calls in
/// proportion to the matches passed, either way, with an `e` offset or not.
#[test]
fn counted_searches_over_rows_list_a_dense_row_once() {
    let row = "a".repeat(16_000);
    for (keys, from, to) in [
        ("4000/\\_a<CR>", (0, 0), (0, 4_000)),
        ("4000/\\_a/e<CR>", (0, 0), (0, 4_000)),
        ("4000?\\_a<CR>", (0, 15_999), (0, 11_999)),
        ("4000?\\_a?e<CR>", (0, 15_999), (0, 11_999)),
    ] {
        let mut vim = HeadlessVim::new(&row, from);
        let before = regex_calls();
        vim.feed(keys).unwrap();
        let calls = regex_calls() - before;
        assert_eq!(vim.cursor(), to, "{keys}");
        assert!(calls <= 2 * 16_000, "{keys}: {calls} regex calls");
    }
}
