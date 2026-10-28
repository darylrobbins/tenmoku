//! The view follows the selection cmd-d or cmd-k cmd-d adds or moves, the
//! primary one, even when it wraps to above the others.

use std::path::Path;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::*;

const ROWS: u32 = 500;

/// `foo` on the first row and on `foo_row`, `r1`, `r2`, ... on the rest.
fn file(foo_row: u32) -> String {
    (0..ROWS)
        .map(|row| {
            if row == 0 || row == foo_row {
                "foo\n".to_string()
            } else {
                format!("r{row}\n")
            }
        })
        .collect()
}

/// A focused editor on `text` with the macOS default keymap, in an active
/// window, with `foo` on `row` selected and revealed.
fn editor<'a>(
    text: &str,
    row: u32,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    cx.update(|cx| init_with_keymap(cx, Keymap::from_layers(vec![default.unwrap()])));
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    let start = row_start(&view, row, cx);
    view.update(cx, |view, cx| view.select_and_reveal(start..start + 3, cx));
    cx.run_until_parked();
    (view, cx)
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

/// Each selection's start and end.
fn ranges(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
    view.update(cx, |view, _| {
        view.model
            .selections()
            .into_iter()
            .map(|selection| (selection.range.start, selection.range.end))
            .collect()
    })
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

#[gpui::test]
fn wrapped_cmd_d_scrolls_to_the_match(cx: &mut TestAppContext) {
    let last = ROWS - 1;
    for (key, kept) in [("cmd-d", true), ("cmd-k cmd-d", false)] {
        let (view, cx) = editor(&file(last), last, cx);
        let bottom = row_start(&view, last, cx);
        assert_eq!(ranges(&view, cx), [(bottom, bottom + 3)], "{key}");
        assert!(!visible_buffer_rows(&view, cx).contains(&0), "{key}");
        assert!(visible_buffer_rows(&view, cx).contains(&last), "{key}");
        keys(key, cx);
        let expected = if kept {
            vec![(0, 3), (bottom, bottom + 3)]
        } else {
            vec![(0, 3)]
        };
        assert_eq!(ranges(&view, cx), expected, "{key}");
        assert!(visible_buffer_rows(&view, cx).contains(&0), "{key}");
    }

    // Without a wrap, the view follows down as before.
    let (view, cx) = editor(&file(400), 0, cx);
    assert!(!visible_buffer_rows(&view, cx).contains(&400));
    keys("cmd-d", cx);
    let match_start = row_start(&view, 400, cx);
    assert_eq!(ranges(&view, cx), [(0, 3), (match_start, match_start + 3)]);
    assert!(visible_buffer_rows(&view, cx).contains(&400));
}
