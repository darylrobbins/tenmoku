//! The `:ls` picker lays its rows out across the popover: each row spans
//! the list's width, the list is as tall as its rows, and "Buffers" shows
//! once.

use std::path::Path;

use fe_ui::ROW_HEIGHT;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::picker::PickerDelegate as _;

use super::open_root;
use crate::Workbench;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A vim window on `root` with `files` open, and `:ls` shown and drawn.
fn ls<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    files: &[String],
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    for file in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "one\ntwo\n").unwrap();
    }
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for file in files {
        let path = root.join(file);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ls");
    cx.simulate_keystrokes("enter");
    draw(cx);
    (workbench, cx)
}

fn long_name() -> String {
    format!("{}file.txt", "a_very_long_directory_name/".repeat(12))
}

#[gpui::test]
fn selected_row_spans_the_list_width(cx: &mut TestAppContext) {
    for name in [long_name(), "a.txt".to_owned()] {
        let dir = tempfile::tempdir().unwrap();
        let (workbench, cx) = ls(cx, dir.path(), &[name]);
        let picker = workbench
            .read_with(cx, |workbench, cx| workbench.vim_pick(cx))
            .unwrap();
        let (list, row) = picker.read_with(cx, |picker, _| {
            (picker.list_bounds(), picker.row_bounds(0).unwrap())
        });
        assert_eq!(row.size.width, list.size.width);
        // A click just inside the list's right edge lands on the row.
        let edge = gpui::point(list.right() - gpui::px(3.), list.center().y);
        cx.simulate_click(edge, gpui::Modifiers::none());
        draw(cx);
        assert!(workbench.read_with(cx, |workbench, cx| workbench.vim_pick(cx).is_none()));
    }
}

#[gpui::test]
fn list_holds_every_row(cx: &mut TestAppContext) {
    for count in [1, 3] {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<String> = (0..count).map(|ix| format!("f{ix}.txt")).collect();
        let (workbench, cx) = ls(cx, dir.path(), &files);
        let picker = workbench
            .read_with(cx, |workbench, cx| workbench.vim_pick(cx))
            .unwrap();
        picker.read_with(cx, |picker, _| {
            let list = picker.list_bounds();
            assert_eq!(list.size.height, ROW_HEIGHT * count as f32);
            for ix in 0..count {
                let row = picker.row_bounds(ix).unwrap();
                assert_eq!(list.union(&row), list);
            }
        });
    }
}

#[gpui::test]
fn buffers_label_shows_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = ls(cx, dir.path(), &["a.txt".to_owned()]);
    let picker = workbench
        .read_with(cx, |workbench, cx| workbench.vim_pick(cx))
        .unwrap();
    let status = picker.read_with(cx, |picker, _| {
        picker.delegate.status().map(|status| status.to_string())
    });
    assert_ne!(status.as_deref(), Some("Buffers"));
}

/// `:marks` rows, which share the picker, span the list too.
#[gpui::test]
fn marks_rows_span_the_list(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = ls(cx, dir.path(), &["a.txt".to_owned()]);
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    cx.simulate_keystrokes("m a");
    cx.simulate_keystrokes(":");
    cx.simulate_input("marks");
    cx.simulate_keystrokes("enter");
    draw(cx);
    let picker = workbench
        .read_with(cx, |workbench, cx| workbench.vim_pick(cx))
        .unwrap();
    picker.read_with(cx, |picker, _| {
        let list = picker.list_bounds();
        let row = picker.row_bounds(0).unwrap();
        assert_eq!(row.size.width, list.size.width);
    });
}
