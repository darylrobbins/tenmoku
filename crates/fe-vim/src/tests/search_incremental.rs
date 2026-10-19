//! A row's matches listed only as far as a search needs (T565): offset
//! searches ([`offset::nth_match`]) and counted searches ([`Rows`], through
//! [`Pattern::nth_by`]) keep a match found before a slow search for the
//! next one, going back stop once no match left to list can come first, and
//! a row cut short by the deadline is never taken as listed.

use std::time::{Duration, Instant};

use fe_text::Text;

use super::*;
use crate::{Search, SearchOffset, SearchPattern, cmdline::offset};

/// After a `b`, the second branch backtracks through the `a`s that follow
/// for far longer than any deadline here.
const SLOW: &str = r"b\|\%(a\|.\)*c";

/// `x`, then `b`s, a blank and the `a`s.
fn slow_text(bs: usize) -> Text {
    Text::new(&format!("x\n{} {}", "b".repeat(bs), "a".repeat(40)))
}

fn short() -> Deadline {
    Deadline::within(Duration::from_millis(50))
}

/// Takes at most a second, well inside the default time limit.
fn quick<T>(run: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let found = run();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    found
}

#[test]
fn a_row_is_listed_as_far_as_asked_and_no_further() {
    let (text, pattern) = (slow_text(2), Pattern::parse(SLOW).unwrap());
    let mut row = RowList::new(&text, 1);
    let found = quick(|| {
        row.list_until(&pattern, &Deadline::new(), |found, _| found.len() == 2)
            .map(<[_]>::to_vec)
    });
    assert_eq!(found, Ok(vec![2..3, 3..4]));
    assert!(!row.done());
    // Listing on hits the slow search: the deadline stops it, the matches
    // found stay, and the row is still unfinished, however often it's asked.
    for _ in 0..2 {
        assert_eq!(
            row.list_until(&pattern, &short(), |_, _| false),
            Err(Interrupted)
        );
        assert_eq!(row.listed(), [2..3, 3..4]);
        assert!(!row.done());
    }
}

/// A row whose last search finds nothing, but only after the deadline, is
/// not taken as listed (T421 F4): the clock is read again after it.
#[test]
fn a_last_search_past_the_deadline_leaves_the_row_unfinished() {
    let text = Text::new(&"a".repeat(8_000_000));
    let pattern = Pattern::parse(r"\d\a").unwrap();
    assert!(matches!(pattern.engine, Engine::Regex { .. }));
    let mut row = RowList::new(&text, 0);
    let deadline = Deadline::within(Duration::from_millis(1));
    assert_eq!(
        row.list_until(&pattern, &deadline, |_, _| false),
        Err(Interrupted)
    );
    assert!(!row.done());
    let mut row = RowList::new(&text, 0);
    assert_eq!(
        row.list_until(&pattern, &Deadline::new(), |_, _| false),
        Ok(&[][..])
    );
    assert!(row.done());
}

/// A search that finds a match, but only after the deadline, counts for
/// nothing: the match isn't listed and the row isn't taken as done.
#[test]
fn a_match_found_past_the_deadline_counts_for_nothing() {
    let text = Text::new(&format!("{}1a", "a".repeat(8_000_000)));
    let pattern = Pattern::parse(r"\d\a").unwrap();
    let mut row = RowList::new(&text, 0);
    let deadline = Deadline::within(Duration::from_millis(1));
    let first = |found: &[Range<usize>], _| !found.is_empty();
    assert_eq!(row.list_until(&pattern, &deadline, first), Err(Interrupted));
    assert!(row.listed().is_empty());
    assert!(!row.done());
    let found = row.list_until(&pattern, &Deadline::new(), first);
    assert_eq!(found.map(<[_]>::first), Ok(Some(&(8_000_000..8_000_002))));
    let deadline = Deadline::within(Duration::from_millis(1));
    assert_eq!(pattern.nth_by(&text, 0, true, 1, &deadline), None);
}

/// Going back, a row is listed only until no match left to list can start
/// before where the search goes from, so the slow search after it is never
/// made.
#[test]
fn searches_back_list_rows_only_as_far_as_they_go_from() {
    let pattern = Pattern::parse(SLOW).unwrap();
    let text = slow_text(2);
    let found = quick(|| pattern.try_find_by(&text, 4, false, &Deadline::new()));
    assert_eq!(found, Ok(Some(3..4)));
    let found = quick(|| pattern.nth_by(&text, 4, false, 2, &Deadline::new()));
    assert_eq!(found, Some(2));
    let deadline = Deadline::new();
    let mut rows = Rows {
        pattern: &pattern,
        text: &text,
        deadline: &deadline,
        listed: HashMap::new(),
    };
    assert_eq!(quick(|| rows.step(3, false)), Ok(2));
    assert_eq!(rows.listed[&1].listed(), [2..3, 3..4]);
}

/// A counted search steps to the next match on a row without waiting on the
/// slow search after it, and gives up at the deadline rather than taking the
/// row as listed and going round to its first match.
#[test]
fn counted_searches_list_rows_as_far_as_a_step_needs() {
    let (text, pattern) = (slow_text(2), Pattern::parse(SLOW).unwrap());
    let found = quick(|| pattern.nth_by(&text, 0, true, 2, &Deadline::new()));
    assert_eq!(found, Some(3));
    assert_eq!(pattern.nth_by(&text, 0, true, 3, &short()), None);
    let deadline = short();
    let mut rows = Rows {
        pattern: &pattern,
        text: &text,
        deadline: &deadline,
        listed: HashMap::new(),
    };
    assert_eq!(rows.step(2, true), Ok(3));
    assert_eq!(rows.step(3, true), Err(Interrupted));
    assert!(!rows.listed[&1].done());
}

/// An offset search takes a row's first match without listing the rest,
/// and gives up at the deadline rather than going round to it again.
#[test]
fn offset_searches_list_rows_as_far_as_a_step_needs() {
    let pattern = Pattern::parse(SLOW).unwrap();
    for (offset, bs) in [(SearchOffset::End(0), 1), (SearchOffset::Start(0), 2)] {
        let text = slow_text(bs);
        let search = |forward| Search {
            pattern: SearchPattern::new(SLOW),
            forward,
            offset,
            again: false,
            word: false,
            ignore_case: false,
        };
        let found =
            quick(|| offset::nth_match(&text, &pattern, 0, search(true), bs, &Deadline::new()));
        assert_eq!(found, Some((1 + bs..2 + bs, false)), "{offset:?}");
        let found = offset::nth_match(&text, &pattern, 0, search(true), bs + 1, &short());
        assert_eq!(found, None, "{offset:?}");
        // Going back from the row's end needs its last match: all of them.
        let found = offset::nth_match(&text, &pattern, text.len(), search(false), 1, &short());
        assert_eq!(found, None, "{offset:?}");
    }
}

/// Going back, an offset search stops listing a row once no match left to
/// list can come before where it goes from: by start, any that start there
/// or later; with an `e` offset, by end, any that start after it, so an
/// empty match there (`\>` after the `b`) still counts. (Without one, an `e`
/// search back from the blank can't tell that no empty match starts there
/// but by the slow search.)
#[test]
fn offset_searches_back_list_rows_only_as_far_as_they_go_from() {
    let text = slow_text(1);
    let boundary = r"b\|\>\|\%(a\|.\)*c";
    for (source, offset, found) in [
        (SLOW, SearchOffset::Start(0), 2..3),
        (boundary, SearchOffset::End(0), 3..3),
    ] {
        let pattern = Pattern::parse(source).unwrap();
        let search = Search {
            pattern: SearchPattern::new(source),
            forward: false,
            offset,
            again: false,
            word: false,
            ignore_case: false,
        };
        let landed = quick(|| offset::nth_match(&text, &pattern, 3, search, 1, &Deadline::new()));
        assert_eq!(landed, Some((found, false)), "{source} {offset:?}");
    }
}
