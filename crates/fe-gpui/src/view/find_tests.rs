//! In-file find: the find pattern's matches painted, stepped through and
//! counted in the background.

use std::ops::Range;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_vim::search::{FindQuery, Pattern};
use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};

use super::find::FindCount;
use crate::*;

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

fn set_find(view: &Entity<EditorView>, text: Option<&str>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.set_find(text.map(pattern), cx));
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The match rects a frame painted, and how many matches it looked at.
fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Vec<Bounds<Pixels>>, usize) {
    crate::element::CANDIDATES.with(|n| n.set(usize::MAX));
    draw(cx);
    let looked_at = crate::element::CANDIDATES.with(|n| n.get());
    let painted = view.read_with(cx, |view, _| view.painted_matches.clone());
    (painted, looked_at)
}

fn shown_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.frame.rows.iter().map(|row| row.buffer_row).collect()
    })
}

fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Range<usize>, usize) {
    view.read_with(cx, |view, _| {
        let selection = view.model().primary_selection();
        (selection.range.clone(), selection.head())
    })
}

fn edit(view: &Entity<EditorView>, at: usize, text: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.model_mut().set_cursor(at);
        view.model_mut().insert(text);
    });
}

/// How many match ranges the last frame found.
fn found() -> usize {
    crate::element::FOUND.with(|n| n.get())
}

/// Replaces the whole text with `text`, and asks for a frame.
fn replace(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let len = view.model().text().len();
        view.model_mut().select_range(0..len, false);
        view.model_mut().insert(text);
        cx.notify();
    });
}

fn count(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<FindCount> {
    view.read_with(cx, |view, _| view.find_count())
}

#[gpui::test]
fn highlights_visible_rows(cx: &mut TestAppContext) {
    let (view, cx) = editor(&"foo\n".repeat(3_000), cx);
    assert_eq!(painted(&view, cx).0, []);

    set_find(&view, Some("foo"), cx);
    let (shown, looked_at) = painted(&view, cx);
    let rows = shown_rows(&view, cx);
    assert!(rows.len() < 3_000, "{}", rows.len());
    // One match on each row in view, and none looked at off it.
    assert_eq!(shown.len(), rows.len());
    assert_eq!(looked_at, rows.len());
    assert_eq!(found(), rows.len());

    // Cleared: nothing looked at, nothing painted.
    set_find(&view, None, cx);
    crate::element::CANDIDATES.with(|n| n.set(0));
    assert_eq!(painted(&view, cx).0, []);
    assert_eq!(count(&view, cx), None);

    // An edit adding a match shows on the next frame.
    set_find(&view, Some("foo"), cx);
    let before = painted(&view, cx).0.len();
    edit(&view, 0, "foo", cx);
    view.update(cx, |_, cx| cx.notify());
    let (shown, _) = painted(&view, cx);
    assert_eq!(shown.len(), before + 1);

    // In a tiny font, with many on each row, far more than the cap's worth
    // of matches are in view.
    view.update(cx, |view, _| view.theme.font_size = px(3.));
    replace(&view, &format!("{}\n", "foo ".repeat(50)).repeat(3_000), cx);
    let (shown, looked_at) = painted(&view, cx);
    assert!(shown_rows(&view, cx).len() * 50 > crate::element::MAX_MATCHES);
    assert_eq!(shown.len(), crate::element::MAX_MATCHES);
    assert!(looked_at <= crate::element::MAX_MATCHES, "{looked_at}");
    assert!(found() <= crate::element::MAX_MATCHES, "{}", found());

    // One row dense with matches: no more than the cap are found.
    replace(&view, &"foo ".repeat(20_000), cx);
    let (shown, _) = painted(&view, cx);
    assert!(!shown.is_empty());
    assert!(shown.len() <= crate::element::MAX_MATCHES);
    assert!(found() <= crate::element::MAX_MATCHES, "{}", found());
}

/// Rows `r0`… with `foo` on rows 5 and 400.
fn file() -> String {
    (0..500)
        .map(|row| match row {
            5 | 400 => format!("r{row} foo\n"),
            _ => format!("r{row}\n"),
        })
        .collect()
}

#[gpui::test]
fn next_previous_and_from(cx: &mut TestAppContext) {
    let (view, cx) = editor(&file(), cx);
    let at = |row: u32, cx: &mut VisualTestContext| {
        let start = view.read_with(cx, |view, _| view.model().text().row_start(row))
            + format!("r{row} ").len();
        start..start + 3
    };
    let (five, four_hundred) = (at(5, cx), at(400, cx));
    let step = |forward: bool, cx: &mut VisualTestContext| {
        let found = view.update(cx, |view, cx| match forward {
            true => view.find_next(cx),
            false => view.find_previous(cx),
        });
        draw(cx);
        found
    };

    // No pattern: nothing happens.
    assert!(!step(true, cx));
    assert_eq!(selection(&view, cx), (0..0, 0));

    set_find(&view, Some("foo"), cx);
    assert!(step(true, cx));
    assert_eq!(selection(&view, cx), (five.clone(), five.end));
    assert!(step(true, cx));
    assert_eq!(
        selection(&view, cx),
        (four_hundred.clone(), four_hundred.end)
    );
    assert!(shown_rows(&view, cx).contains(&400));
    // Wrapping past the end, and back past the start.
    assert!(step(true, cx));
    assert_eq!(selection(&view, cx).0, five);
    assert!(shown_rows(&view, cx).contains(&5));
    assert!(step(false, cx));
    assert_eq!(selection(&view, cx).0, four_hundred);
    assert!(step(false, cx));
    assert_eq!(selection(&view, cx).0, five);

    // From an offset: a match starting there, else the next, wrapping.
    let from = |offset: usize, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| view.find_from(offset, cx))
    };
    assert!(from(four_hundred.start, cx));
    assert_eq!(selection(&view, cx).0, four_hundred);
    assert!(from(four_hundred.start + 1, cx));
    assert_eq!(selection(&view, cx).0, five);

    // No match: false, and the selection stays.
    set_find(&view, Some("zzz"), cx);
    assert!(!step(true, cx));
    assert!(!step(false, cx));
    assert!(!from(0, cx));
    assert_eq!(selection(&view, cx).0, five);
    set_find(&view, None, cx);
    assert!(!from(0, cx));
    assert_eq!(selection(&view, cx).0, five);

    // With vim on, the caret goes to the match's start.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    set_find(&view, Some("foo"), cx);
    assert!(step(true, cx));
    let start = four_hundred.start;
    assert_eq!(selection(&view, cx), (start..start, start));
    assert!(from(0, cx));
    assert_eq!(selection(&view, cx), (five.start..five.start, five.start));
}

#[gpui::test]
fn count_off_thread(cx: &mut TestAppContext) {
    let (view, cx) = editor(&"foo foo foo\n".repeat(4), cx);
    set_find(&view, Some("foo"), cx);
    assert_eq!(count(&view, cx), None);
    cx.run_until_parked();
    let counted = |current| {
        Some(FindCount {
            total: 12,
            capped: false,
            current,
        })
    };
    assert_eq!(count(&view, cx), counted(None));
    view.update(cx, |view, cx| view.find_next(cx));
    assert_eq!(count(&view, cx), counted(Some(2)));
    view.update(cx, |view, cx| view.find_previous(cx));
    view.update(cx, |view, cx| view.find_previous(cx));
    assert_eq!(count(&view, cx), counted(Some(12)));
    // With vim on, the match at the caret.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    view.update(cx, |view, cx| view.find_from(4, cx));
    assert_eq!(count(&view, cx), counted(Some(2)));
    cx.update(|_, cx| set_vim_enabled(cx, false));

    // An edit while counting: the stale count is dropped, the recount has
    // the edit, whether or not a frame comes between.
    set_find(&view, Some("foo"), cx);
    edit(&view, 0, "foo ", cx);
    assert_eq!(count(&view, cx), None);
    cx.run_until_parked();
    assert_eq!(count(&view, cx).map(|count| count.total), Some(13));
    edit(&view, 0, "foo ", cx);
    assert_eq!(count(&view, cx), None);
    view.update(cx, |_, cx| cx.notify());
    draw(cx);
    edit(&view, 0, "foo ", cx);
    cx.run_until_parked();
    assert_eq!(count(&view, cx).map(|count| count.total), Some(15));

    // An edit through another view of the buffer, after a count landed:
    // the one frame that catches up with it starts the recount.
    let mut other = view.read_with(cx, |view, _| view.model().split());
    other.set_cursor(0);
    other.insert("foo ");
    let version = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.layout.as_ref().unwrap().version)
    };
    let drawn = version(cx);
    // The notify draws a frame, which catches up with the edit.
    view.update(cx, |_, cx| cx.notify());
    assert_ne!(version(cx), drawn);
    assert!(view.read_with(cx, |view, _| view.is_counting()));
    cx.run_until_parked();
    assert_eq!(count(&view, cx).map(|count| count.total), Some(16));

    // Past the cap, over more than one chunk of rows.
    replace(&view, &"foo foo foo foo\n".repeat(5_000), cx);
    assert_eq!(count(&view, cx), None);
    draw(cx);
    cx.run_until_parked();
    let capped = count(&view, cx).unwrap();
    assert_eq!((capped.total, capped.capped), (10_000, true));
}

#[cfg(test)]
mod peer_edit_tests;
