//! A highlight query that runs out of time isn't cached as if it were
//! complete: it asks to be drawn again, and the next call queries the range
//! with twice the time, up to a cap where what it found is kept.

use std::{ops::Range, time::Duration};

use fe_buffer::Buffer;

use super::parsed;
use crate::{
    HighlightSpan, Syntax, language_by_name,
    syntax::{DEADLINE_AFTER, DEADLINE_CHECKS},
};

/// Runs `f` with every deadline passing after `after` checks, and counts the
/// checks.
fn with_deadline_after<T>(after: usize, f: impl FnOnce() -> T) -> (T, usize) {
    DEADLINE_CHECKS.set(0);
    DEADLINE_AFTER.set(after);
    let result = f();
    DEADLINE_AFTER.set(usize::MAX);
    (result, DEADLINE_CHECKS.get())
}

/// About 2,000 rows of TOML, as in a pipeline config, and the range of
/// 60 rows in its middle, a screenful.
fn pipeline() -> (Buffer, Range<usize>) {
    let block = "[[pipeline.stages]]\nname = \"test\"\n";
    let text = block.repeat(1000);
    let start = 500 * block.len();
    (Buffer::new(&text), start..start + 30 * block.len())
}

fn toml(buffer: &Buffer) -> Syntax {
    parsed(language_by_name("toml").unwrap(), buffer)
}

/// The range's highlights, queried to the end.
fn full(syntax: &Syntax, range: Range<usize>) -> Vec<HighlightSpan> {
    let split = syntax.language().local_patterns;
    syntax.query(syntax.tree().unwrap(), range, split, Duration::MAX)
}

/// How many deadline checks highlighting `range` afresh makes, with the
/// deadline passing at half of them, so the query stops mid-range.
fn half_the_checks(buffer: &Buffer, range: Range<usize>) -> usize {
    let syntax = toml(buffer);
    let (_, checks) = with_deadline_after(usize::MAX, || syntax.highlights(range));
    assert!(checks >= 4, "only {checks} deadline checks");
    checks / 2
}

/// The screenshot's bug: the top rows coloured and the rest plain, for as
/// long as the view stays put, because the cut-short result was cached.
#[test]
fn a_query_cut_short_is_not_cached() {
    let (buffer, range) = pipeline();
    let after = half_the_checks(&buffer, range.clone());
    let syntax = toml(&buffer);
    let full = full(&syntax, range.clone());

    let (cut, _) = with_deadline_after(after, || syntax.highlights(range.clone()));
    assert!(
        !cut.is_empty() && cut.len() < full.len(),
        "{} of {} spans: not cut short mid-range",
        cut.len(),
        full.len()
    );
    assert_eq!(syntax.highlights(range.clone()), full);
}

/// fe-gpui draws again when `take_missing_languages` returns anything, so a
/// cut-short call returns the host language there, and a complete one
/// nothing.
#[test]
fn a_cut_short_query_asks_for_a_redraw() {
    let (buffer, range) = pipeline();
    let after = half_the_checks(&buffer, range.clone());
    let syntax = toml(&buffer);

    with_deadline_after(after, || syntax.highlights(range.clone()));
    assert!(syntax.take_missing_languages().contains(&"toml"));
    syntax.highlights(range.clone());
    assert_eq!(syntax.take_missing_languages(), Vec::<&str>::new());
}

/// A range that runs out of 2, 4, 8 and then 16 ms keeps what the last
/// query found, so it isn't queried and drawn again on every frame.
#[test]
fn a_range_cut_short_at_the_cap_is_cached() {
    let (buffer, range) = pipeline();
    let after = half_the_checks(&buffer, range.clone());
    let syntax = toml(&buffer);
    let full = full(&syntax, range.clone());

    for budget_ms in [2, 4, 8] {
        let (cut, _) = with_deadline_after(after, || syntax.highlights(range.clone()));
        assert!(cut.len() < full.len(), "{budget_ms} ms: not cut short");
        assert_eq!(
            syntax.take_missing_languages(),
            vec!["toml"],
            "{budget_ms} ms: no redraw asked for"
        );
    }
    let (capped, _) = with_deadline_after(after, || syntax.highlights(range.clone()));
    assert!(capped.len() < full.len(), "16 ms: not cut short");
    assert_eq!(syntax.take_missing_languages(), Vec::<&str>::new());

    let (cached, checks) = with_deadline_after(after, || syntax.highlights(range.clone()));
    assert_eq!((cached, checks), (capped, 0));
    assert_eq!(syntax.take_missing_languages(), Vec::<&str>::new());
}

/// A frame of folded rows can query more ranges than the cache keeps
/// results for. Cycling through twelve of them, each still gets twice the
/// time on each round, up to the cap, after which no round asks for a
/// redraw, though the cache drops their results and they're queried again.
#[test]
fn cycling_more_ranges_than_the_cache_still_reaches_the_cap() {
    let (buffer, screen) = pipeline();
    let ranges: Vec<Range<usize>> = (0..12)
        .map(|ix| screen.start - ix * screen.len()..screen.end - ix * screen.len())
        .collect();
    let afters: Vec<usize> = ranges
        .iter()
        .map(|range| half_the_checks(&buffer, range.clone()))
        .collect();
    let syntax = toml(&buffer);
    let round = || {
        for (range, &after) in ranges.iter().zip(&afters) {
            let (cut, _) = with_deadline_after(after, || syntax.highlights(range.clone()));
            assert!(cut.len() < full(&syntax, range.clone()).len());
        }
        syntax.take_missing_languages()
    };

    for budget_ms in [2, 4, 8] {
        assert_eq!(round(), vec!["toml"], "{budget_ms} ms: no redraw asked for");
    }
    for round_ix in 4..8 {
        assert_eq!(round(), Vec::<&str>::new(), "round {round_ix}");
    }
}
