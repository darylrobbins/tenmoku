//! Merge staging while it waits in the git queue: its editors are checked
//! again as their turn comes, and files are read in their own encodings.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, path::Path, process::Command};

use gpui::TestAppContext;

use crate::Workbench;

use super::{Action, Group, click_button, draw, git, open, open_file, read, toasts, write};

/// A repository stopped on a merge whose `a.rs` conflicts.
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

/// `text` as UTF-16 little-endian with a byte order mark.
fn utf16(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

fn release_all(cx: &mut gpui::VisualTestContext) {
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    while cx.update(|_, cx| Workbench::git_work_held(cx)) > 0 {
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
    }
}

#[gpui::test]
fn a_retargeted_editor_aborts_the_queued_merge_staging(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    let editor = open_file(&workbench, root, "a.rs", cx);
    let before = read(root, "a.rs");
    // Earlier git work holds the queue while the stage is asked for.
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    click_button(&panel, Some(1), Group::Changes, Action::Stage, cx);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    cx.simulate_prompt_answer("Stage");
    draw(cx);
    let moved = root.join("moved.rs");
    editor.update(cx, |editor, _| {
        editor.model_mut().set_path(Some(moved.clone()))
    });
    release_all(cx);
    assert!(unmerged(root).contains("a.rs"), "still unmerged");
    assert_eq!(read(root, "a.rs"), before);
    assert!(!moved.exists(), "nothing was saved");
    let shown = toasts(&workbench, cx);
    assert!(
        (shown.iter()).any(|(message, _)| message.contains("no longer edits a.rs")),
        "{shown:?}"
    );
}

#[gpui::test]
fn a_utf16_file_with_restored_markers_still_asks(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let markers = read(root, "a.rs");
    fs::write(root.join("a.rs"), utf16(&markers)).unwrap();

    // On disk at the first check.
    let (workbench, panel, cx) = open(cx, root);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(
        question,
        "Are you sure you want to stage a.rs with merge conflicts?"
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(unmerged(root).contains("a.rs"));

    // Restored while staging waits.
    let editor = open_file(&workbench, root, "a.rs", cx);
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
    release_all(cx);
    let (question, _) = cx.pending_prompt().expect("asked again");
    assert_eq!(
        question,
        "Are you sure you want to stage a.rs with merge conflicts?"
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(unmerged(root).contains("a.rs"), "still unmerged");
    let shown = toasts(&workbench, cx);
    assert!(
        (shown.iter()).any(|(message, _)| message.contains("now has merge conflicts")),
        "{shown:?}"
    );
}

#[gpui::test]
#[cfg(unix)]
fn an_unreadable_file_aborts_merge_staging_naming_it(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    let path = root.join("a.rs");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    draw(cx);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!cx.has_pending_prompt());
    assert!(unmerged(root).contains("a.rs"));
    let shown = toasts(&workbench, cx);
    assert!(
        (shown.iter()).any(|(message, _)| message.contains("can't read a.rs")),
        "{shown:?}"
    );
}

#[gpui::test]
fn a_disk_only_utf16_file_with_restored_markers_asks_again(cx: &mut TestAppContext) {
    let dir = conflicted();
    let root = dir.path();
    let markers = read(root, "a.rs");
    fs::write(root.join("a.rs"), utf16("one\nboth\nthree\n")).unwrap();
    let (workbench, panel, cx) = open(cx, root);
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    click_button(&panel, Some(1), Group::Changes, Action::Stage, cx);
    click_button(&panel, Some(0), Group::Merge, Action::Stage, cx);
    assert!(!cx.has_pending_prompt());
    fs::write(root.join("a.rs"), utf16(&markers)).unwrap();
    release_all(cx);
    let (question, _) = cx.pending_prompt().expect("asked again");
    assert_eq!(
        question,
        "Are you sure you want to stage a.rs with merge conflicts?"
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(unmerged(root).contains("a.rs"), "still unmerged");
    let shown = toasts(&workbench, cx);
    assert!(
        (shown.iter()).any(|(message, _)| message.contains("now has merge conflicts")),
        "{shown:?}"
    );
}
