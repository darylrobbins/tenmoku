//! The Changes panel's Merge Changes group, on a repository stopped on a
//! real conflict from `git merge`.

use std::{path::Path, process::Command};

use fe_git::ChangeKind;
use gpui::TestAppContext;

use crate::Workbench;

use super::{
    Action, Group, ShownGroup, click_button, draw, focus, git, index_text, keys, open, open_file,
    read, shown_groups, write,
};

/// A repository stopped on a merge whose `a.rs` conflicts, with an
/// unrelated edit to `b.rs` in the working tree.
fn conflicted() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    write(root, "a.rs", "one\ntwo\nthree\n");
    write(root, "b.rs", "b\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "side"]);
    write(root, "a.rs", "one\nside\nthree\n");
    git(root, &["commit", "-q", "-am", "side"]);
    git(root, &["checkout", "-q", "main"]);
    write(root, "a.rs", "one\nmain\nthree\n");
    git(root, &["commit", "-q", "-am", "main"]);
    let merged = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["merge", "side"])
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(!merged.status.success(), "the merge should conflict");
    write(root, "b.rs", "edited\n");
    dir
}

fn unmerged(root: &Path) -> String {
    let output = Command::new("git")
        .args(["ls-files", "-u"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn group(title: &'static str, rows: &[(&'static str, &str)]) -> ShownGroup {
    let count = rows.len();
    let rows = (rows.iter())
        .map(|(letter, path)| (*letter, (*path).to_owned()))
        .collect();
    (title, count, rows)
}

#[gpui::test]
fn conflicted_files_are_in_merge_changes_first(cx: &mut TestAppContext) {
    let dir = conflicted();
    let (_, panel, cx) = open(cx, dir.path());
    assert_eq!(
        shown_groups(&panel, cx),
        [
            group("Merge Changes", &[("C", "a.rs")]),
            group("Changes", &[("M", "b.rs")]),
        ]
    );
    let (merge, changes) = panel.read_with(cx, |panel, _| {
        let header = |group| panel.header_bounds(group).expect("drawn");
        (header(Group::Merge), header(Group::Changes))
    });
    assert!(merge.bottom() <= changes.top());
    let tree: Vec<_> = panel.read_with(cx, |panel, _| {
        panel.statuses().map(|(_, kind)| kind).collect()
    });
    assert_eq!(tree, [ChangeKind::Conflicted, ChangeKind::Modified]);
}

#[gpui::test]
fn staging_a_file_with_markers_asks_first(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let (_, panel, cx) = open(cx, root);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(
        question,
        "Are you sure you want to stage a.rs with merge conflicts?"
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(unmerged(root).contains("a.rs"));

    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    cx.simulate_prompt_answer("Stage");
    draw(cx);
    assert!(unmerged(root).is_empty());
    let shown = shown_groups(&panel, cx);
    let titles: Vec<_> = shown.iter().map(|group| group.0).collect();
    assert_eq!(titles, ["Staged Changes", "Changes"]);
    assert_eq!(shown[0].2, [("M", "a.rs".to_owned())]);
    assert!(index_text(root, "a.rs").unwrap().contains("<<<<<<<"));
}

#[gpui::test]
fn a_resolved_file_stages_without_asking(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    write(root, "a.rs", "one\nboth\nthree\n");
    let (_, panel, cx) = open(cx, root);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    assert!(!cx.has_pending_prompt());
    assert!(unmerged(root).is_empty());
    assert_eq!(
        index_text(root, "a.rs").as_deref(),
        Some("one\nboth\nthree\n")
    );
    assert_eq!(read(root, "a.rs"), "one\nboth\nthree\n");
}

#[gpui::test]
fn a_merge_row_opens_the_file(cx: &mut TestAppContext) {
    let dir = conflicted();
    let (workbench, panel, cx) = open(cx, dir.path());
    focus(&panel, cx);
    keys("down", cx);
    keys("enter", cx);
    let (is_editor, is_diff) = workbench.read_with(cx, |workbench, cx| {
        let item = workbench.pane().read(cx).active_item().expect("a tab");
        (
            item.downcast_ref::<fe_gpui::EditorView>().is_some(),
            item.downcast_ref::<fe_gpui::DiffView>().is_some(),
        )
    });
    assert!(is_editor && !is_diff);
}

#[gpui::test]
fn an_unsaved_resolution_is_saved_before_it_is_staged(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    let editor = open_file(&workbench, root, "a.rs", cx);
    editor.update(cx, |editor, _| {
        editor.model_mut().select_all();
        editor.model_mut().insert("one\nboth\nthree\n");
    });
    draw(cx);
    assert!(read(root, "a.rs").contains("<<<<<<<"), "not saved yet");
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    assert!(!cx.has_pending_prompt());
    assert!(unmerged(root).is_empty());
    assert_eq!(
        index_text(root, "a.rs").as_deref(),
        Some("one\nboth\nthree\n")
    );
    assert_eq!(read(root, "a.rs"), "one\nboth\nthree\n");
}

#[gpui::test]
fn an_open_buffer_with_markers_asks_and_is_staged_as_saved(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    open_file(&workbench, root, "a.rs", cx);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    assert!(cx.pending_prompt().is_some());
    cx.simulate_prompt_answer("Stage");
    draw(cx);
    assert!(unmerged(root).is_empty());
    assert!(index_text(root, "a.rs").unwrap().contains("<<<<<<<"));
}

#[gpui::test]
fn markers_back_before_the_save_are_not_staged_unasked(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    let editor = open_file(&workbench, root, "a.rs", cx);
    let markers = editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string());
    editor.update(cx, |editor, _| {
        editor.model_mut().select_all();
        editor.model_mut().insert("one\nboth\nthree\n");
    });
    draw(cx);
    // Earlier git work holds the queue while the stage is asked for.
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    click_button(&panel, Some(1), Group::Changes, Action::Stage, cx);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    assert!(!cx.has_pending_prompt());
    editor.update(cx, |editor, _| {
        editor.model_mut().select_all();
        editor.model_mut().insert(&markers);
    });
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    while cx.update(|_, cx| Workbench::git_work_held(cx)) > 0 {
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
    }
    assert!(unmerged(root).contains("a.rs"), "still unmerged");
    let shown = super::toasts(&workbench, cx);
    assert!(
        shown
            .iter()
            .any(|(message, _)| message.contains("now has merge conflicts")),
        "{shown:?}"
    );
}
