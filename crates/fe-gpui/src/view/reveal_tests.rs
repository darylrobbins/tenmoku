use std::ops::Range;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::*;

const ROWS: usize = 500;

/// An editor on a file of `ROWS` rows, `r0`, `r1`, ..., scrolled to the top.
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

fn file() -> String {
    (0..ROWS).map(|row| format!("r{row}\n")).collect()
}

fn row_start(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> usize {
    view.update(cx, |view, _| view.model.text().row_start(row))
}

fn visible_buffer_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.frame.rows.iter().map(|row| row.buffer_row).collect()
    })
}

fn selections(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, usize, usize)> {
    view.update(cx, |view, _| {
        view.model
            .selections()
            .iter()
            .map(|s| (s.range.start, s.range.end, s.head()))
            .collect()
    })
}

#[gpui::test]
fn reveals_an_offscreen_range(cx: &mut TestAppContext) {
    let (view, cx) = editor(&file(), cx);
    assert!(!visible_buffer_rows(&view, cx).contains(&400));
    let start = row_start(&view, 400, cx);
    view.update(cx, |view, cx| view.select_and_reveal(start..start + 3, cx));
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), [(start, start + 3, start + 3)]);
    assert!(visible_buffer_rows(&view, cx).contains(&400));
}

#[gpui::test]
fn clamps_stale_offsets(cx: &mut TestAppContext) {
    let text = "ab\nc\u{e9}d\n";
    let (view, cx) = editor(text, cx);
    let len = text.len();

    view.update(cx, |view, cx| view.select_and_reveal(len - 1..len + 50, cx));
    assert_eq!(selections(&view, cx), [(len - 1, len, len)]);

    view.update(cx, |view, cx| {
        view.select_and_reveal(len + 10..len + 20, cx)
    });
    assert_eq!(selections(&view, cx), [(len, len, len)]);

    // `é` spans bytes 4..6; 5 is inside it.
    view.update(cx, |view, cx| view.select_and_reveal(5..5, cx));
    assert_eq!(selections(&view, cx), [(4, 4, 4)]);

    let (start, end) = (7, 1);
    view.update(cx, |view, cx| {
        view.select_and_reveal(Range { start, end }, cx)
    });
    assert_eq!(selections(&view, cx), [(1, 7, 7)]);
    cx.run_until_parked();
}

#[gpui::test]
fn unfolds_a_hidden_target(cx: &mut TestAppContext) {
    let (view, cx) = editor(&file(), cx);
    view.update(cx, |view, _| {
        view.model.fold_rows(10..=40);
        view.model.fold_rows(100..=120);
    });
    cx.run_until_parked();
    let target = row_start(&view, 20, cx);
    view.update(cx, |view, cx| view.select_and_reveal(target..target, cx));
    cx.run_until_parked();
    let folds: Vec<(u32, u32)> = view.update(cx, |view, _| {
        view.model
            .display()
            .folds()
            .iter()
            .map(|f| (f.start, f.end))
            .collect()
    });
    assert_eq!(folds.len(), 1, "{folds:?}");
    assert!(folds[0].0 >= 99, "{folds:?}");
    assert!(visible_buffer_rows(&view, cx).contains(&20));
}
