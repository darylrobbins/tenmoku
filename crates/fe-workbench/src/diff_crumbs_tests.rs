//! The pane toolbar's crumbs over a diff view: the compared file's path,
//! else the tab's title; never `untitled`.

use std::fs;

use fe_ui::CrumbKind;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{dispatch, draw, fixture, git, press, short, window};
use crate::Workbench;

/// The labels and kinds of the crumbs the pane drew.
fn crumbs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, CrumbKind)> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.drawn_crumbs().iter())
            .map(|(label, kind, _)| (label.clone(), *kind))
            .collect()
    })
}

#[gpui::test]
fn a_working_tree_diffs_crumbs_are_its_files_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/conf.toml"), "a = 1\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("src/conf.toml"), "a = 2\n").unwrap();
    let (workbench, mut cx) = window(cx, root, "src/conf.toml");
    let cx = &mut cx;

    let expected = vec![
        ("src".to_owned(), CrumbKind::Path),
        ("conf.toml".to_owned(), CrumbKind::File),
    ];
    for id in ["git.diff_index", "git.diff_head"] {
        dispatch(id, cx);
        let title = workbench.read_with(cx, |workbench, cx| {
            let item = workbench.pane().read(cx).active_item().unwrap();
            item.tab_title(cx).to_string()
        });
        assert!(title.contains('↔'), "{id}: {title}");
        assert_eq!(crumbs(&workbench, cx), expected, "{id}");
    }
}

#[gpui::test]
fn a_commit_diffs_crumb_is_its_tab_title(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "b.rs");
    let cx = &mut cx;
    dispatch("git.file_history", cx);
    press("enter", cx);
    let s = short(&ids[0]);
    let title = format!("b.rs ({s}^ ↔ {s})");
    assert_eq!(crumbs(&workbench, cx), vec![(title, CrumbKind::File)]);
}

#[gpui::test]
fn an_unsaved_editor_is_still_untitled(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "b.rs");
    let cx = &mut cx;
    dispatch("workbench.new_file", cx);
    draw(cx);
    let untitled = vec![("untitled".to_owned(), CrumbKind::File)];
    assert_eq!(crumbs(&workbench, cx), untitled);
}
