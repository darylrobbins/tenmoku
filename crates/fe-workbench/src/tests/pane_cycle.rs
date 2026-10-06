//! Cycling tabs with a single tab open brings it back from a file being
//! read or a read error, as it does with several.

use std::{fs, path::PathBuf};

use fe_project::fixture;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{click_row, open_root, panel};
use crate::Workbench;

const KEYS: [&str; 2] = ["cmd-shift-]", "cmd-shift-["];

/// A project with `a.txt` and `b.txt` at its root, and their paths.
fn project() -> (tempfile::TempDir, [PathBuf; 2]) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 1).unwrap();
    let paths = ["a.txt", "b.txt"].map(|name| {
        let path = dir.path().join(name);
        fs::write(&path, name).unwrap();
        path
    });
    (dir, paths)
}

/// The open tabs' paths and the active tab's index; the shown editor's
/// path, if an editor is shown; and whether that editor has focus.
fn state(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<PathBuf>, Option<usize>, Option<PathBuf>, bool) {
    let (paths, active, editor) = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let paths = pane.tabs().iter().map(|tab| tab.path.clone().unwrap());
        (
            paths.collect(),
            pane.active(),
            workbench.active_editor(cx).cloned(),
        )
    });
    let shown = editor
        .as_ref()
        .map(|editor| editor.read_with(cx, |editor, _| editor.model().path().unwrap().to_owned()));
    let focused = editor
        .is_some_and(|editor| cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    (paths, active, shown, focused)
}

fn open_file(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
}

#[gpui::test]
fn cycling_one_tab_clears_a_pending_read(cx: &mut TestAppContext) {
    let (dir, [a, b]) = project();
    for key in KEYS {
        let (workbench, cx) = open_root(cx, dir.path());
        click_row(&panel(&workbench, cx), "a.txt", cx);
        open_file(&workbench, b.clone(), cx);
        assert_eq!(
            state(&workbench, cx),
            (vec![a.clone()], Some(0), None, false)
        );

        // The key is handled before b's read lands, which then opens unshown.
        cx.simulate_keystrokes(key);
        assert_eq!(
            state(&workbench, cx),
            (vec![a.clone(), b.clone()], Some(0), Some(a.clone()), true),
            "{key}"
        );
    }
}

#[gpui::test]
fn cycling_one_tab_clears_a_read_error(cx: &mut TestAppContext) {
    let (dir, [a, _]) = project();
    for key in KEYS {
        let (workbench, cx) = open_root(cx, dir.path());
        click_row(&panel(&workbench, cx), "a.txt", cx);
        // Reading a directory fails.
        open_file(&workbench, dir.path().join(fixture::dir_name(0)), cx);
        cx.run_until_parked();
        assert_eq!(
            state(&workbench, cx),
            (vec![a.clone()], Some(0), None, false)
        );

        cx.simulate_keystrokes(key);
        assert_eq!(
            state(&workbench, cx),
            (vec![a.clone()], Some(0), Some(a.clone()), true),
            "{key}"
        );
    }
}
