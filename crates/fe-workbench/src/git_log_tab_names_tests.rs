//! Tabs of same-named files, diffs from the Git Log or the files
//! themselves, told apart by the folders that differ, drawn after the
//! title.

use std::fs;

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::PaneView as _;

use super::{commit, draw, focus_panel, git, panel, press, window, write};
use crate::Workbench;

/// A repository whose last commit, `edit`, changes `src/ex.rs` and
/// `src/tests/ex.rs`. The commit's id.
fn fixture() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::create_dir_all(root.join("src/tests")).unwrap();
    write(root, "src/ex.rs", "one\n");
    write(root, "src/tests/ex.rs", "test one\n");
    commit(root, "init");
    write(root, "src/ex.rs", "two\n");
    write(root, "src/tests/ex.rs", "test two\n");
    let edit = commit(root, "edit");
    (dir, edit)
}

/// Each tab's title and description, as the pane draws them.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>)> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx).pane();
        (pane.tabs(cx).iter().zip(pane.items()))
            .map(|(tab, item)| {
                let description = tab.description_text().map(ToString::to_string);
                (item.tab_title(cx).to_string(), description)
            })
            .collect()
    })
}

#[gpui::test]
fn two_commit_diffs_of_same_named_files_show_their_folders(cx: &mut TestAppContext) {
    let (dir, edit) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    panel.update(cx, |panel, cx| panel.select(0, cx));
    focus_panel(&workbench, cx);
    press("enter", cx);
    let lines = panel.read_with(cx, |panel, _| panel.lines());
    assert_eq!(lines[2..4], ["M src/ex.rs", "M src/tests/ex.rs"]);

    press("j j enter", cx);
    focus_panel(&workbench, cx);
    press("j enter", cx);
    let s = &edit[..7];
    let title = format!("ex.rs ({s}^ ↔ {s})");
    assert_eq!(
        tabs(&workbench, cx),
        [
            (title.clone(), Some("src".to_owned())),
            (title, Some("tests".to_owned())),
        ]
    );
}

#[gpui::test]
fn two_file_tabs_of_same_named_files_show_their_folders(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for file in ["src/ex.rs", "src/tests/ex.rs"] {
        let path = dir.path().join(file);
        pane.update_in(cx, |pane, window, cx| pane.open(path, window, cx));
        draw(cx);
    }
    assert_eq!(
        tabs(&workbench, cx),
        [
            ("ex.rs".to_owned(), Some("src".to_owned())),
            ("ex.rs".to_owned(), Some("tests".to_owned())),
        ]
    );
}
