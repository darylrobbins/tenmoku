//! The find's scope: drawn as a band, under the matches, which are only those
//! inside it.

// A scope of one range is what these mean, not a mistyped `Vec` of offsets.
#![allow(clippy::single_range_in_vec_init)]

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use fe_buffer::{Buffer, Text};
use fe_editor::EditorModel;
use fe_vim::search::{Deadline, FindQuery, Pattern, matches_in, regex_calls, set_call_limit};
use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext};

use super::{MAX_COUNTED, count_in_scope, kept_on_row};
use crate::{element::TEST_LIMIT, *};

fn editor<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    (view, cx)
}

fn pattern(text: &str) -> Pattern {
    Pattern::from_query(&FindQuery {
        text: text.into(),
        match_case: true,
        ..FindQuery::default()
    })
    .unwrap()
}

fn regex(text: &str) -> Pattern {
    Pattern::from_query(&FindQuery {
        text: text.into(),
        regex: true,
        ..FindQuery::default()
    })
    .unwrap()
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Sets `query`'s find, scoped to `selected`, selected for the capture and
/// then moved away from.
fn scoped(
    view: &Entity<EditorView>,
    query: Option<&str>,
    selected: Range<usize>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        view.model_mut().select_range(selected, false);
        let scope = view.selection_scope();
        assert!(scope.is_some());
        view.set_find_scoped(query.map(pattern), scope, cx);
        view.model_mut().select_range(0..0, false);
    });
    draw(cx);
}

fn bands(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    view.read_with(cx, |view, _| view.find.painted_scope.clone())
}

fn matches(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    view.read_with(cx, |view, _| view.painted_matches.clone())
}

#[gpui::test]
fn scope_is_painted(cx: &mut TestAppContext) {
    let (view, cx) = editor("foo one\nfoo two\nfoo three\nfoo four\n", cx);
    draw(cx);
    assert_eq!(bands(&view, cx), []);

    // A band on each row it covers, with no query yet too.
    scoped(&view, None, 8..24, cx);
    let shown = bands(&view, cx);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(shown[0].origin.y < shown[1].origin.y);
    assert_eq!(matches(&view, cx), []);

    // The matches are those inside it, painted over it.
    crate::element::decorations::PAINTED.with(|painted| painted.borrow_mut().clear());
    scoped(&view, Some("foo"), 8..24, cx);
    let painted = crate::element::decorations::PAINTED.with(|painted| painted.take());
    let first = |stage: &str| {
        painted
            .iter()
            .position(|painted| *painted == stage)
            .unwrap()
    };
    assert!(first("find scope") < first("matches"));
    let found = matches(&view, cx);
    assert_eq!(found.len(), 2, "{found:?}");
    let shown = bands(&view, cx);
    assert_eq!(shown[0].origin, found[0].origin);
    assert_eq!(shown[1].origin, found[1].origin);

    // Gone with the scope.
    view.update(cx, |view, cx| view.set_find(Some(pattern("foo")), cx));
    draw(cx);
    assert_eq!(bands(&view, cx), []);
    assert_eq!(matches(&view, cx).len(), 4);
}

#[gpui::test]
fn the_count_and_minimap_matches_are_the_scopes(cx: &mut TestAppContext) {
    let (view, cx) = editor("foo\nfoo\nfoo\nfoo\n", cx);
    scoped(&view, Some("foo"), 4..11, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.find_counted(), Some(&[4..7, 8..11][..]));
    });
}

#[gpui::test]
fn whole_rows_selected_leave_out_the_next_row(cx: &mut TestAppContext) {
    let (view, cx) = editor("a\nb\nc\n", cx);
    let start = regex("^");
    view.update(cx, |view, cx| {
        view.model_mut().select_range(2..4, false);
        let scope = view.selection_scope();
        view.set_find_scoped(Some(start), scope, cx);
    });
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.find_scope(), Some(vec![2..4]));
        assert_eq!(view.find_counted(), Some(&[2..2][..]));
    });
}

#[gpui::test]
fn a_blank_row_selected_is_a_scope(cx: &mut TestAppContext) {
    let (view, cx) = editor("\nfoo\n", cx);
    let start = regex("^");
    view.update(cx, |view, cx| {
        view.model_mut().select_range(0..1, false);
        let scope = view.selection_scope();
        assert!(scope.is_some());
        view.set_find_scoped(Some(start), scope, cx);
    });
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.find_scope(), Some(vec![0..1]));
        assert_eq!(view.find_counted(), Some(&[0..0][..]));
    });
}

#[gpui::test]
fn text_inserted_at_the_row_after_a_whole_row_scope_stays_out(cx: &mut TestAppContext) {
    let (view, cx) = editor("a\nb\nc\n", cx);
    scoped(&view, None, 2..4, cx);
    view.update(cx, |view, _| {
        view.model_mut().select_range(4..4, false);
        view.model_mut().insert("x");
    });
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.find_scope(), Some(vec![2..4]));
    });
}

/// A row of `count` matches of `a`.
fn dense(count: usize) -> (Text, Pattern) {
    (Text::new(&"a".repeat(count)), pattern("a"))
}

#[test]
fn stepping_through_a_dense_row_goes_no_further_than_it_needs() {
    let (text, pattern) = dense(50_000);
    let scope = [10..50_000];
    let before = regex_calls();
    let found = EditorView::next_in_find_scope(
        &text,
        &pattern,
        &scope,
        100,
        |found| found.start > 100,
        &mut Deadline::never(),
    );
    assert_eq!(found, Ok(Some(101..102)));
    // Up to the wanted one, from the row's start, and no further.
    assert!(regex_calls() - before < 150, "{}", regex_calls() - before);
}

#[test]
fn a_match_found_counts_only_while_the_deadline_stands() {
    let next = |limit| {
        let (text, pattern) = dense(4);
        set_call_limit(Some(limit));
        let found = EditorView::next_in_find_scope(
            &text,
            &pattern,
            &[0..4],
            0,
            |_| true,
            &mut Deadline::new(),
        );
        set_call_limit(None);
        found
    };
    // One call finds it, so a budget of two has room for that, and no more
    // are made; one has run out by the time it's found.
    assert_eq!(next(2), Ok(Some(0..1)));
    assert_eq!(next(1), Err(fe_vim::search::Interrupted));
}

#[test]
fn going_back_on_a_dense_row_stops_at_where_it_started() {
    let (text, pattern) = dense(50_000);
    let before = regex_calls();
    let found = EditorView::previous_in_find_scope(
        &text,
        &pattern,
        &[0..50_000],
        100,
        &mut Deadline::never(),
    );
    assert_eq!(found, Ok(Some(99..100)));
    assert!(regex_calls() - before < 110, "{}", regex_calls() - before);
}

#[test]
fn going_back_cut_short_says_so_rather_than_settling_for_an_earlier_match() {
    let (text, pattern) = dense(100);
    set_call_limit(Some(5));
    let found =
        EditorView::previous_in_find_scope(&text, &pattern, &[0..100], 50, &mut Deadline::new());
    set_call_limit(None);
    assert_eq!(found, Err(fe_vim::search::Interrupted));
}

#[test]
fn a_count_on_a_dense_row_stops_at_the_cap() {
    let (text, pattern) = dense(50_000);
    let before = regex_calls();
    let (ranges, capped) = count_in_scope(&text, &pattern, &[0..50_000], || false).unwrap();
    assert_eq!((ranges.len(), capped), (MAX_COUNTED, true));
    assert!(regex_calls() - before < MAX_COUNTED as u64 + 50);
}

/// A `stop` that says so after `polls` asks.
fn after_polls(polls: usize) -> impl Fn() -> bool + Send + 'static {
    let asked = Arc::new(AtomicUsize::new(0));
    move || asked.fetch_add(1, Ordering::Relaxed) >= polls
}

#[test]
fn a_count_stops_when_told_partway_through_what_it_leaves_out() {
    // A dense prefix before the scope, each match of it left out.
    let (text, pattern) = dense(50_000);
    let before = regex_calls();
    let counted = count_in_scope(&text, &pattern, &[49_999..50_000], after_polls(5));
    assert_eq!(counted, None);
    assert!(regex_calls() - before < 10, "{}", regex_calls() - before);

    // Candidates rejected before a match comes of them: none is a whole word.
    let whole = Pattern::from_query(&FindQuery {
        text: "a".into(),
        whole_word: true,
        ..FindQuery::default()
    })
    .unwrap();
    let before = regex_calls();
    let counted = count_in_scope(&text, &whole, &[0..50_000], after_polls(5));
    assert_eq!(counted, None);
    assert!(regex_calls() - before < 10, "{}", regex_calls() - before);
}

#[test]
fn what_follows_the_scope_on_its_row_isnt_gone_through() {
    // Nothing after `foo` is a whole word: candidates a search would reject
    // one by one to the end of the row.
    let text = Text::new(&format!("foo {}", "a".repeat(50_000)));
    let whole = Pattern::from_query(&FindQuery {
        text: "foo|a".into(),
        regex: true,
        whole_word: true,
        ..FindQuery::default()
    })
    .unwrap();
    let scope = [0..3];
    let before = regex_calls();
    let (ranges, capped) = count_in_scope(&text, &whole, &scope, || false).unwrap();
    assert_eq!((ranges, capped), (vec![0..3], false));
    let found =
        EditorView::next_in_find_scope(&text, &whole, &scope, 0, |_| true, &mut Deadline::never());
    assert_eq!(found, Ok(Some(0..3)));
    assert!(regex_calls() - before < 10, "{}", regex_calls() - before);
}

#[test]
fn stopping_at_the_scopes_end_finds_what_a_whole_search_would() {
    let text = Text::new("aaa foo bar\n\nfoo\n  x foo\n");
    let rows = text.row_count();
    let patterns = ["x*", "^", "$", "foo", "o*", r"\b", "a|foo", ".*"].map(regex);
    let scopes: [&[Range<usize>]; 5] = [
        &[0..1],
        &[0..3, 4..7],
        &[2..9, 12..14],
        &[0..text.len()],
        &[5..6, 14..18],
    ];
    for pattern in &patterns {
        let whole = matches_in(&text, 0..rows, pattern, usize::MAX);
        for scope in scopes {
            let want: Vec<_> = (whole.iter())
                .filter(|found| EditorView::in_find_scope(&text, scope, found))
                .cloned()
                .collect();
            let (ranges, _) = count_in_scope(&text, pattern, scope, || false).unwrap();
            assert_eq!(ranges, want, "{:?} in {scope:?}", pattern.regex_str());
        }
    }
}

#[test]
fn only_as_many_are_searched_as_are_kept() {
    // Only every fourth is kept, and it goes no further than the third of them.
    let (text, pattern) = dense(1_000);
    let before = regex_calls();
    let found = kept_on_row(
        &text,
        0,
        0..1_000,
        &pattern,
        3,
        &mut Deadline::never(),
        |found| found.start % 4 == 0,
    );
    assert_eq!(found, [0..1, 4..5, 8..9]);
    assert_eq!(regex_calls() - before, 9);
}

#[test]
fn a_range_ending_at_a_rows_start_doesnt_search_that_row() {
    let text = Text::new(&format!("foo\n{}", "a".repeat(1_000)));
    let pattern = regex("foo|a");
    let scope = [0..4];

    let before = regex_calls();
    let (ranges, _) = count_in_scope(&text, &pattern, &scope, || false).unwrap();
    assert_eq!(ranges, [0..3]);
    let found = EditorView::next_in_find_scope(
        &text,
        &pattern,
        &scope,
        4,
        |found| found.start >= 4,
        &mut Deadline::never(),
    );
    assert_eq!(found, Ok(Some(0..3)));
    assert!(regex_calls() - before < 10, "{}", regex_calls() - before);

    let rows: Vec<_> = EditorView::find_scope_rows(&text, &scope).collect();
    assert_eq!(rows, [(0, 0..3)]);
    assert_eq!(
        EditorView::find_scope_window(&text, &scope, &(4..1_004)),
        None
    );
}

#[test]
fn ranges_sharing_a_row_list_it_once() {
    let text = Text::new("foo foo foo\nfoo\nfoo\n");
    let scope = [0..3, 8..14, 16..19];
    let rows: Vec<_> = EditorView::find_scope_rows(&text, &scope).collect();
    assert_eq!(rows, [(0, 0..11), (1, 12..14), (2, 16..19)]);
}

#[gpui::test]
fn rows_outside_the_scope_arent_searched_for_a_frame(cx: &mut TestAppContext) {
    let (view, cx) = editor(&"foo foo foo\n".repeat(30), cx);
    scoped(&view, Some("foo"), 12..23, cx);
    let before = regex_calls();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let calls = regex_calls() - before;
    assert_eq!(matches(&view, cx).len(), 3);
    assert!(calls < 10, "{calls}");
}

#[gpui::test]
fn a_new_scope_searches_again_after_a_frame_ran_out_of_time(cx: &mut TestAppContext) {
    let (view, cx) = editor("foo\nfoo\nfoo\n", cx);
    let ran_out = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.layout.as_ref().expect("drawn").highlight_ran_out()
        })
    };
    TEST_LIMIT.with(|limit| limit.set(Some(Duration::ZERO)));
    scoped(&view, Some("foo"), 0..7, cx);
    assert!(ran_out(&view, cx));
    TEST_LIMIT.with(|limit| limit.set(Some(Duration::from_secs(3600))));
    draw(cx);
    assert_eq!(matches(&view, cx), [], "latched");

    // Another scope with the same query: searched afresh.
    scoped(&view, Some("foo"), 4..11, cx);
    assert!(!ran_out(&view, cx));
    assert_eq!(matches(&view, cx).len(), 2);

    // The same with none, after running out again.
    TEST_LIMIT.with(|limit| limit.set(Some(Duration::ZERO)));
    scoped(&view, Some("foo"), 0..7, cx);
    assert!(ran_out(&view, cx));
    TEST_LIMIT.with(|limit| limit.set(Some(Duration::from_secs(3600))));
    view.update(cx, |view, cx| view.set_find(Some(pattern("foo")), cx));
    draw(cx);
    assert_eq!(matches(&view, cx).len(), 3);
    TEST_LIMIT.with(|limit| limit.set(None));
}

#[test]
fn inside_is_wholly_inside() {
    let text = Text::new(&"x".repeat(20));
    let scope = [2..5, 8..9];
    let inside = |found: Range<usize>| EditorView::in_find_scope(&text, &scope, &found);
    assert!(inside(2..5));
    assert!(inside(3..3));
    assert!(inside(5..5));
    assert!(inside(8..9));
    assert!(!inside(1..3));
    assert!(!inside(4..6));
    assert!(!inside(2..9));
    assert!(!inside(6..6));
    assert!(!inside(9..10));
}
