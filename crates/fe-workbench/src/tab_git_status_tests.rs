//! An open tab's title in its file's git status colour, from the Changes
//! panel's list: set when the list lands and when a tab opens.

use std::{fs, path::Path};

use fe_git::{ChangeKind, FileStatus};
use fe_gpui::EditorView;
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem as _;

use crate::{GitPanel, Workbench, git_panel, tests::open_root};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn changes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    })
}

fn set_statuses(
    workbench: &Entity<Workbench>,
    root: &Path,
    statuses: &[(&str, ChangeKind)],
    cx: &mut VisualTestContext,
) {
    let statuses = (statuses.iter())
        .map(|(path, kind)| FileStatus {
            path: root.join(path),
            kind: *kind,
        })
        .collect();
    let changes = changes(workbench, cx);
    changes.update(cx, |changes, cx| changes.set_statuses(statuses, cx));
    draw(cx);
}

fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    })
}

fn tint(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Hsla> {
    editor.read_with(cx, |editor, cx| editor.tab_title_color(cx))
}

fn color(kind: ChangeKind, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Hsla {
    workbench.read_with(cx, |_, cx| {
        git_panel::letter_color(kind, &fe_ui::ui_theme(cx))
    })
}

fn folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.rs", "n.rs", "b.rs"] {
        fs::write(dir.path().join(name), "fn main() {}\n").unwrap();
    }
    dir
}

#[gpui::test]
fn tabs_take_their_files_status_colour(cx: &mut TestAppContext) {
    let dir = folder();
    let (workbench, cx) = open_root(cx, dir.path());
    let [a, n, b] =
        ["a.rs", "n.rs", "b.rs"].map(|name| open_file(&workbench, &dir.path().join(name), cx));
    set_statuses(
        &workbench,
        dir.path(),
        &[
            ("a.rs", ChangeKind::Modified),
            ("n.rs", ChangeKind::Untracked),
        ],
        cx,
    );
    assert_eq!(
        tint(&a, cx),
        Some(color(ChangeKind::Modified, &workbench, cx))
    );
    assert_eq!(
        tint(&n, cx),
        Some(color(ChangeKind::Untracked, &workbench, cx))
    );
    assert_eq!(tint(&b, cx), None);

    set_statuses(&workbench, dir.path(), &[], cx);
    for editor in [&a, &n, &b] {
        assert_eq!(tint(editor, cx), None);
    }
}

#[gpui::test]
fn a_tab_opened_after_the_status_landed_is_tinted_at_once(cx: &mut TestAppContext) {
    let dir = folder();
    let (workbench, cx) = open_root(cx, dir.path());
    set_statuses(
        &workbench,
        dir.path(),
        &[("a.rs", ChangeKind::Modified)],
        cx,
    );
    let a = open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert_eq!(
        tint(&a, cx),
        Some(color(ChangeKind::Modified, &workbench, cx))
    );
}

#[gpui::test]
fn a_tab_opened_through_another_spelling_of_its_path_is_tinted(cx: &mut TestAppContext) {
    let dir = folder();
    let alias = tempfile::tempdir().unwrap();
    let link = alias.path().join("link");
    std::os::unix::fs::symlink(dir.path(), &link).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    set_statuses(
        &workbench,
        dir.path(),
        &[("a.rs", ChangeKind::Modified)],
        cx,
    );
    let viaed = open_file(&workbench, &link.join("a.rs"), cx);
    assert_eq!(
        tint(&viaed, cx),
        Some(color(ChangeKind::Modified, &workbench, cx))
    );
}

#[gpui::test]
fn deleted_files_opened_through_other_spellings_keep_their_tint(cx: &mut TestAppContext) {
    let dir = folder();
    let alias = tempfile::tempdir().unwrap();
    let link = alias.path().join("link");
    std::os::unix::fs::symlink(dir.path(), &link).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let name = dir.path().file_name().unwrap();
    let dotdot = dir.path().join("..").join(name);
    let a = open_file(&workbench, &link.join("a.rs"), cx);
    let b = open_file(&workbench, &dotdot.join("b.rs"), cx);
    for name in ["a.rs", "b.rs"] {
        fs::remove_file(dir.path().join(name)).unwrap();
    }
    set_statuses(
        &workbench,
        dir.path(),
        &[("a.rs", ChangeKind::Deleted), ("b.rs", ChangeKind::Deleted)],
        cx,
    );
    let deleted = color(ChangeKind::Deleted, &workbench, cx);
    assert_eq!(tint(&a, cx), Some(deleted));
    assert_eq!(tint(&b, cx), Some(deleted));
}

#[gpui::test]
fn a_listed_symlink_keeps_its_own_status_apart_from_its_target(cx: &mut TestAppContext) {
    let dir = folder();
    std::os::unix::fs::symlink(dir.path().join("a.rs"), dir.path().join("l.rs")).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let target = open_file(&workbench, &dir.path().join("a.rs"), cx);
    let link = open_file(&workbench, &dir.path().join("l.rs"), cx);
    set_statuses(
        &workbench,
        dir.path(),
        &[
            ("a.rs", ChangeKind::Modified),
            ("l.rs", ChangeKind::Untracked),
        ],
        cx,
    );
    assert_eq!(
        tint(&target, cx),
        Some(color(ChangeKind::Modified, &workbench, cx))
    );
    assert_eq!(
        tint(&link, cx),
        Some(color(ChangeKind::Untracked, &workbench, cx))
    );
    // Only the target listed: the symlink does not take its status.
    set_statuses(
        &workbench,
        dir.path(),
        &[("a.rs", ChangeKind::Modified)],
        cx,
    );
    assert_eq!(tint(&link, cx), None);
}
