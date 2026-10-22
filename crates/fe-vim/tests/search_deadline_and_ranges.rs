//! Searches stop at their deadline, counted in regex calls or time, with
//! nothing found by a call that overran it; a collection's `\n` as a
//! range's end and marks after `\_.` match as nvim's do (positions probed
//! with `searchpos()` in `nvim --clean --headless`, nvim 0.12.4); and a
//! backward count over rows with and without matches lists each row once.
//!
//! Each test gives its search a `Deadline` of its own. A budget of regex
//! calls is process-wide, so these run as nextest does, a test a process.

use std::ops::Range;
use std::time::Duration;

use fe_text::Text;
use fe_vim::HeadlessVim;
use fe_vim::search::{Deadline, Interrupted, Pattern, regex_calls, set_call_limit};

/// `pattern`'s match from `from` in `buffer`, with no deadline.
fn found(pattern: &str, buffer: &str, from: usize, forward: bool) -> Option<Range<usize>> {
    let pattern = Pattern::parse_across(pattern, false).unwrap();
    let text = Text::new(buffer);
    pattern
        .find_across(&text, from, forward, &Deadline::never())
        .unwrap()
}

/// A search whose budget runs out on its last regex call, whether that call
/// found the match or ruled out the last row, is interrupted: what the call
/// found counts for nothing. One more call and it finds what it would with
/// no deadline.
#[test]
fn an_interrupted_search_returns_interrupted_not_a_result() {
    let text = Text::new("abc\ndef\nabc\ndef");
    for (pattern, from, forward) in [
        // Within a row: found on a later row, and found on none.
        ("e", 0, true),
        ("x", 0, true),
        ("b", 14, false),
        ("x", 14, false),
        // Over rows.
        (r"c\nd", 0, true),
        (r"c\nd", 14, false),
        (r"x\ny", 0, true),
        (r"f\_s*a", 14, false),
    ] {
        let parsed = Pattern::parse_across(pattern, false).unwrap();
        let before = regex_calls();
        let unlimited = parsed.find_across(&text, from, forward, &Deadline::never());
        let calls = regex_calls() - before;
        assert!(calls > 0, "{pattern}");
        set_call_limit(Some(calls));
        let at_budget = parsed.find_across(&text, from, forward, &Deadline::new());
        set_call_limit(Some(calls + 1));
        let within = parsed.find_across(&text, from, forward, &Deadline::new());
        set_call_limit(None);
        let case = (pattern, from, forward);
        assert_eq!(at_budget, Err(Interrupted), "{case:?}");
        assert_eq!(within, unlimited, "{case:?}");
    }
}

/// A search whose deadline has passed stops before reading its window,
/// even one that holds the rest of the text for a pattern with no bound on
/// the rows it takes: it makes no regex call and gives `Interrupted`.
#[test]
fn window_loading_stops_at_the_deadline() {
    let text = Text::new(&vec!["a b c"; 20_000].join("\n"));
    let pattern = Pattern::parse_across(r"b\_s\+x", false).unwrap();
    for (from, forward) in [(6, true), (6, false), (60_000, false)] {
        let deadline = Deadline::after(Duration::ZERO);
        let before = regex_calls();
        assert_eq!(
            pattern.find_across(&text, from, forward, &deadline),
            Err(Interrupted)
        );
        assert_eq!(regex_calls(), before, "from {from}, forward {forward}");
        assert!(deadline.ran_out());
    }
}

/// `[\n-a]` is the range from the character 0x0a to `a`, not a line break
/// and the members `-` and `a`: on "x\n5\na" it takes the `5`. nvim:
/// `searchpos('[\n-a]')` is [2, 1].
#[test]
fn a_newline_range_endpoint_is_the_character() {
    let buffer = "x\n5\na";
    assert_eq!(found(r"[\n-a]", buffer, buffer.len(), true), Some(2..3));
    // From the `x`, forward: the `5` too.
    assert_eq!(found(r"[\n-a]", buffer, 0, true), Some(2..3));
    // `\_[\n-a]` adds the line break: the first after the `x`.
    assert_eq!(found(r"\_[\n-a]", buffer, buffer.len(), true), Some(1..2));
}

/// `\_.` followed by U+0301 is a character carrying the mark, never a bare
/// line break: on "ab\ncd" there's no match. nvim: `searchpos('\_.́')` is
/// [0, 0].
#[test]
fn any_or_newline_keeps_its_composing_mark() {
    for from in [0, 2, 5] {
        for forward in [true, false] {
            assert_eq!(found("\\_.\u{301}", "ab\ncd", from, forward), None);
        }
    }
    // Before a character carrying it, the line break matches with that
    // character, as in nvim: [1, 2].
    assert_eq!(found("\\_.\u{301}", "a\nb\u{301}", 0, true), Some(1..5));
}

/// `4000?a\_s*` from the `z` after 4,000 rows of `a` each followed by one
/// of `x` goes back over every `a`, round to the first, as nvim's does:
/// each row, with a match or without, is listed once for all the count's
/// searches, not again for each.
#[test]
fn a_backward_count_over_rows_lists_each_row_once() {
    let pairs = 4_000;
    let buffer = format!("{}z", "a\nx\n".repeat(pairs));
    let mut vim = HeadlessVim::new(&buffer, (2 * pairs as u32, 0));
    let before = regex_calls();
    vim.feed(&format!("{pairs}?a\\_s*<CR>")).unwrap();
    let calls = regex_calls() - before;
    assert_eq!(vim.cursor(), (0, 0));
    // A call or two a row; each search starting over from the first row
    // took about eight million.
    assert!(calls < 6 * pairs as u64, "{calls} regex calls");
}
