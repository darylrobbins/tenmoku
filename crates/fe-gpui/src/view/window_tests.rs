use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::*;

const ROW: &str = "fn f() {}\n";

/// An editor on `text`, a Rust file over the parse limit, once its language
/// has loaded.
fn editor(text: String, cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    assert!(text.len() > fe_syntax::MAX_PARSE_BYTES);
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

/// Moves the caret to `offset`, scrolling it into view, and lets the
/// background work settle. Returns the start of each visible row and whether
/// a keyword starts it.
fn show(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Vec<(usize, bool)> {
    view.update(cx, |view, cx| {
        view.model.set_cursor(offset);
        view.changed(cx);
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let syntax = view.syntax.as_ref().expect("language loaded");
        assert!(!syntax.is_parsing());
        let rows = &view.layout.as_ref().expect("drawn").frame.rows;
        rows.iter()
            .map(|row| {
                let keyword = syntax
                    .syntax()
                    .highlights(row.start..row.end)
                    .iter()
                    .any(|span| {
                        span.range.start == row.start && span.highlight.name() == "keyword"
                    });
                (row.start, keyword)
            })
            .collect()
    })
}

/// A file too big to parse whole highlights a window around whatever is
/// scrolled into view, parsed in the background.
#[gpui::test]
fn a_huge_file_highlights_around_the_viewport(cx: &mut TestAppContext) {
    let text = ROW.repeat(900_000);
    let middle = text.len() / 2;
    let (view, cx) = editor(text, cx);
    let rows = show(&view, middle, cx);
    assert!(rows.iter().any(|&(start, _)| start == middle));
    assert!(rows.iter().all(|&(_, keyword)| keyword), "{rows:?}");
}

/// Text hidden between visible rows, by a fold or past what a long row
/// shapes, doesn't move the window away from them.
#[gpui::test]
fn hidden_text_doesnt_move_the_window(cx: &mut TestAppContext) {
    let long_row = 600_000 * ROW.len();
    let text = ROW.repeat(600_000) + &"x".repeat(1 << 20) + "\n" + &ROW.repeat(300_000);
    let (view, cx) = editor(text, cx);

    // A 1 MB row near the bottom of the screen.
    let rows = show(&view, long_row + 1_000_001 + 3 * ROW.len(), cx);
    let above: Vec<_> = rows
        .iter()
        .filter(|&&(start, _)| start < long_row)
        .collect();
    assert!(
        above.len() > 10 && above.iter().all(|&&(_, keyword)| keyword),
        "{rows:?}"
    );

    // 3 MB folded under a row near the top, scrolling up to it.
    let header = 100_000 * ROW.len();
    let folded = header..400_000 * ROW.len();
    view.update(cx, |view, _| {
        view.model.select_range(folded.clone(), false);
        view.model.fold(&|_| None);
    });
    let rows = show(&view, header, cx);
    assert!(rows.iter().any(|&(start, _)| start <= header), "{rows:?}");
    let below: Vec<_> = rows
        .iter()
        .filter(|&&(start, _)| start > folded.end)
        .collect();
    assert!(
        below.len() > 10 && below.iter().all(|&&(_, keyword)| keyword),
        "{rows:?}"
    );
}
