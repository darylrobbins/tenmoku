//! Back and forward while a file is still read: the place the caret will be
//! is recorded, whichever way the file was opened, in whichever pane.

use std::{
    fs,
    path::{Path, PathBuf},
};

use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{Workbench, nav::NavOp, pane::Reveal};

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n}\n";
const UTIL: &str = "pub fn helper() {}\n";
const MULTI: &str = "one\ntwo\nthree\n";
const OTHER: &str = "fn other() {}\n";

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    for (rel, text) in [
        ("main", MAIN),
        ("util", UTIL),
        ("other", OTHER),
        ("third", OTHER),
        ("multi", MULTI),
    ] {
        fs::write(root.join("src").join(format!("{rel}.rs")), text).unwrap();
    }
    (dir, root)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench showing `src/util.rs`, jumped to from `src/main.rs` with the
/// caret at 7.
fn jumped<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| {
            pane.open(root.join("src/main.rs"), window, cx)
        });
    });
    settle(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update_in(cx, |workbench, window, cx| {
        let editor = workbench.active_editor(cx).unwrap().clone();
        editor.update(cx, |editor, _| editor.model_mut().set_cursor(7));
        workbench.push_history(cx);
        workbench.open_range(root.join("src/util.rs"), 4..4, window, cx);
    });
    settle(cx);
    (workbench, cx)
}

fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize) {
    workbench.read_with(cx, |workbench, cx| {
        let model = workbench
            .active_editor(cx)
            .expect("a file")
            .read(cx)
            .model();
        (
            model.path().unwrap().to_owned(),
            model.primary_selection().head(),
        )
    })
}

/// Opens `rel` plainly in the active pane, then runs `op`, before any read
/// lands.
fn open_then(
    workbench: &Entity<Workbench>,
    root: &Path,
    rel: &str,
    op: NavOp,
    cx: &mut VisualTestContext,
) {
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(root.join(rel), window, cx));
        workbench.run_nav_op(op, window, cx);
    });
    settle(cx);
}

#[gpui::test]
fn back_and_forward_while_an_ordinary_open_is_read(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = jumped(cx, &root);

    // Back from other.rs, still read, to main.rs; forward returns to it.
    open_then(&workbench, &root, "src/other.rs", NavOp::Back, cx);
    assert_eq!(at(&workbench, cx), (root.join("src/main.rs"), 7));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Forward, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/other.rs"), 0));

    // And the symmetric case: forward from third.rs, still read.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Back, window, cx)
    });
    settle(cx);
    open_then(&workbench, &root, "src/third.rs", NavOp::Forward, cx);
    assert_eq!(at(&workbench, &mut *cx), (root.join("src/other.rs"), 0));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Back, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/third.rs"), 0));
}

#[gpui::test]
fn overlapping_navigation_opens_in_two_splits(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = jumped(cx, &root);
    cx.simulate_keystrokes("cmd-\\");
    settle(cx);
    let (first, second) = workbench.read_with(cx, |workbench, _| {
        let active = workbench.pane().clone();
        let other = workbench
            .pane_group()
            .panes()
            .into_iter()
            .find(|p| **p != active);
        (active, other.expect("a split").clone())
    });

    // The first pane starts navigating to other.rs; before it's read, the
    // second does to third.rs. Each update ends with its effects applied,
    // but no read run.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.push_history(cx);
        workbench.open_range(root.join("src/other.rs"), 3..3, window, cx);
    });
    cx.update(|_, cx| second.update(cx, |_, cx| cx.emit(crate::PaneEvent::Activated)));
    workbench.update_in(cx, |workbench, window, cx| {
        assert_eq!(*workbench.pane(), second);
        workbench.push_history(cx);
        workbench.open_range(root.join("src/third.rs"), 5..5, window, cx);
        // Back from the second's pending place.
        workbench.run_nav_op(NavOp::Back, window, cx);
    });
    cx.update(|_, cx| first.update(cx, |_, cx| cx.emit(crate::PaneEvent::Activated)));
    workbench.update_in(cx, |workbench, window, cx| {
        assert_eq!(*workbench.pane(), first);
        // And from the first's.
        workbench.run_nav_op(NavOp::Back, window, cx);
    });
    settle(cx);

    // Forward returns to both, the later first.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Forward, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/other.rs"), 3));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Forward, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/third.rs"), 5));
}

/// Opens `rel` in the active pane as a search result on row 1, columns
/// 1..2 (offset 6 once read), then runs `op`, before any read lands.
fn search_open_then(
    workbench: &Entity<Workbench>,
    root: &Path,
    op: NavOp,
    cx: &mut VisualTestContext,
) {
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        let reveal = Reveal {
            row: 1,
            columns: 1..2,
            within_row: false,
        };
        pane.update(cx, |pane, cx| {
            pane.open_at(root.join("src/multi.rs"), Some(reveal), window, cx)
        });
        workbench.run_nav_op(op, window, cx);
    });
    settle(cx);
}

#[gpui::test]
fn back_while_a_search_result_on_a_later_row_is_read(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = jumped(cx, &root);

    // Back is deferred, not lost: history is intact once the file lands.
    search_open_then(&workbench, &root, NavOp::Back, cx);
    assert_eq!(at(&workbench, cx), (root.join("src/multi.rs"), 6));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Back, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/main.rs"), 7));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Forward, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/multi.rs"), 6));
}

#[gpui::test]
fn forward_while_a_search_result_on_a_later_row_is_read(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = jumped(cx, &root);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Back, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/main.rs"), 7));

    search_open_then(&workbench, &root, NavOp::Forward, cx);
    assert_eq!(at(&workbench, cx), (root.join("src/multi.rs"), 6));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Forward, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/util.rs"), 4));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_nav_op(NavOp::Back, window, cx)
    });
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/multi.rs"), 6));
}
