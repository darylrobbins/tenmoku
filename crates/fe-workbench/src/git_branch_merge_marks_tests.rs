//! The branch item's `!` while a merge or rebase is in progress, as VS
//! Code's `Repository.headLabel` shows it.

use std::fs;
use std::path::Path;
use std::process::Command;

use gpui::TestAppContext;

use super::{branch_item, fixture, git, init, refresh, window};

/// Runs `git` in `dir`, which is expected to stop on a conflict.
fn conflicts(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_EDITOR", "true")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(!output.status.success(), "git {args:?} conflicts");
}

/// Commits a change to `a.txt` on main, so main and feat conflict.
fn diverge(work: &Path) {
    fs::write(work.join("a.txt"), "main side\n").unwrap();
    git(work, &["commit", "-q", "-am", "main side"]);
}

#[gpui::test]
fn branch_item_marks_a_merge_until_it_is_committed(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    diverge(&work);
    let (workbench, mut vcx) = window(cx, &work);
    let cx = &mut vcx;

    conflicts(&work, &["merge", "-q", "feat"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main!"));

    fs::write(work.join("a.txt"), "resolved\n").unwrap();
    git(&work, &["add", "a.txt"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main+!"));

    git(&work, &["commit", "-q", "--no-edit"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
}

#[gpui::test]
fn branch_item_marks_a_stopped_rebase(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    diverge(&work);
    let (workbench, mut vcx) = window(cx, &work);
    let cx = &mut vcx;

    conflicts(&work, &["rebase", "feat"]);
    refresh(&workbench, cx);
    let item = branch_item(&workbench, cx).unwrap();
    assert!(item.ends_with('!') && !item.ends_with("+!"), "{item}");

    fs::write(work.join("a.txt"), "resolved\n").unwrap();
    git(&work, &["add", "a.txt"]);
    refresh(&workbench, cx);
    let item = branch_item(&workbench, cx).unwrap();
    assert!(item.ends_with("+!"), "{item}");

    git(&work, &["rebase", "--abort"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
}

#[gpui::test]
fn branch_item_marks_no_conflicted_cherry_pick(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    diverge(&work);
    let (workbench, mut vcx) = window(cx, &work);
    let cx = &mut vcx;

    conflicts(&work, &["cherry-pick", "feat"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
}
