//! Searching right after an edit through another view of the buffer, with
//! no frame between.

use super::*;

const TEXT: &str = "a foo b foo";

/// A view with "foo" found and counted, and a peer that has inserted
/// "xx " at the start, no frame drawn since.
fn peered(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = editor(TEXT, cx);
    set_find(&view, Some("foo"), cx);
    cx.run_until_parked();
    let mut peer = view.read_with(cx, |view, _| view.model().split());
    peer.set_cursor(0);
    peer.insert("xx ");
    (view, cx)
}

fn text_at(view: &Entity<EditorView>, range: Range<usize>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        view.model().text().to_string()[range].to_string()
    })
}

#[gpui::test]
fn find_next_and_previous_after_a_peer_edit_select_the_match(cx: &mut TestAppContext) {
    let (view, cx) = peered(cx);
    view.update(cx, |view, cx| view.find_next(cx));
    assert_eq!(selection(&view, cx).0, 5..8);
    assert_eq!(text_at(&view, 5..8, cx), "foo");
    assert_eq!(count(&view, cx), None);
    view.update(cx, |view, cx| view.find_previous(cx));
    assert_eq!(selection(&view, cx).0, 11..14);
    assert_eq!(text_at(&view, 11..14, cx), "foo");
    cx.run_until_parked();
    let found = count(&view, cx).unwrap();
    assert_eq!((found.total, found.current), (2, Some(2)));
}

#[gpui::test]
fn find_from_after_a_peer_edit_selects_the_match(cx: &mut TestAppContext) {
    let (view, cx) = peered(cx);
    view.update(cx, |view, cx| view.find_from(0, cx));
    assert_eq!(selection(&view, cx).0, 5..8);
    assert_eq!(text_at(&view, 5..8, cx), "foo");
}
