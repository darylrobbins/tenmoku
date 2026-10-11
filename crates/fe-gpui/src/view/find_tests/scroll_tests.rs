//! The find pattern's matches painted on rows dense with them, scrolled.

use super::*;

fn dense() -> String {
    "foo ".repeat(20_000)
}

/// The text area's window bounds.
fn text_area(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        layout.bounds(layout.frame.text_bounds())
    })
}

fn inside(rect: &Bounds<Pixels>, area: &Bounds<Pixels>) -> bool {
    rect.left() >= area.left()
        && rect.right() <= area.right()
        && rect.top() >= area.top()
        && rect.bottom() <= area.bottom()
}

#[gpui::test]
fn dense_row_scrolled_right_paints_its_matches(cx: &mut TestAppContext) {
    let (view, cx) = editor(&dense(), cx);
    set_find(&view, Some("foo"), cx);
    view.update(cx, |view, _| {
        view.scroll.x = px(150_000.);
        view.autoscroll = false;
    });
    let (shown, _) = painted(&view, cx);
    assert!(!shown.is_empty());
    let area = text_area(&view, cx);
    assert!(shown.iter().all(|rect| inside(rect, &area)), "{shown:?}");
    assert!(found() <= shown.len() + 64, "{} {}", found(), shown.len());
}

#[gpui::test]
fn dense_wrapped_row_scrolled_down_paints_its_matches(cx: &mut TestAppContext) {
    let (view, cx) = editor(&dense(), cx);
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    set_find(&view, Some("foo"), cx);
    view.update(cx, |view, _| {
        view.scroll.y = px(20_000.);
        view.autoscroll = false;
    });
    let (shown, _) = painted(&view, cx);
    assert_eq!(shown_rows(&view, cx).iter().max(), Some(&0));
    let rows = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.rows.len());
    // Every displayed row is full of "foo ", so each has matches.
    let mut tops: Vec<_> = shown.iter().map(|rect| rect.origin.y).collect();
    tops.dedup();
    assert_eq!(tops.len(), rows);
    assert!(found() <= shown.len() + 64, "{} {}", found(), shown.len());
}

#[gpui::test]
fn rows_after_a_dense_row_paint_their_matches(cx: &mut TestAppContext) {
    let text = format!("{}\n{}", dense(), vec!["foo"; 30].join("\n"));
    let (view, cx) = editor(&text, cx);
    set_find(&view, Some("foo"), cx);
    let (shown, _) = painted(&view, cx);
    let rows = shown_rows(&view, cx);
    assert!(rows.len() > 2);
    // Painted in row order: the groups of rects at one y.
    let mut counts: Vec<usize> = Vec::new();
    let mut last = None;
    for rect in &shown {
        match last == Some(rect.origin.y) {
            true => *counts.last_mut().unwrap() += 1,
            false => counts.push(1),
        }
        last = Some(rect.origin.y);
    }
    assert_eq!(counts.len(), rows.len());
    assert!(counts[1..].iter().all(|&n| n == 1), "{counts:?}");
}

/// The one match is the last character of the bottom displayed row, a
/// wrapped one: it is painted.
#[gpui::test]
fn the_bottom_wrapped_rows_last_character_is_painted(cx: &mut TestAppContext) {
    let (view, cx) = editor(&"a".repeat(20_000), cx);
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    draw(cx);
    let end = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.rows.last().unwrap().end
    });
    replace(
        &view,
        &format!("{}b{}", "a".repeat(end - 1), "a".repeat(20_000)),
        cx,
    );
    set_find(&view, Some("b"), cx);
    let (shown, _) = painted(&view, cx);
    let bottom = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.rows.last().unwrap().end
    });
    assert_eq!(bottom, end);
    assert_eq!(shown.len(), 1);
}

/// A tab straddling the text area's right edge is a match painted, clipped.
#[gpui::test]
fn a_tab_at_the_right_edge_is_painted(cx: &mut TestAppContext) {
    let (view, cx) = editor(&"\t".repeat(2_000), cx);
    set_find(&view, Some("\t"), cx);
    view.update(cx, |view, _| {
        view.scroll.x = px(3.);
        view.autoscroll = false;
    });
    let (shown, _) = painted(&view, cx);
    let area = text_area(&view, cx);
    let last = shown.last().expect("matches painted");
    assert!(last.right() >= area.right() - px(0.01), "{last:?} {area:?}");
    assert!(shown.iter().all(|rect| inside(rect, &area)));
}
