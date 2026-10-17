//! The branch item's `*`, `+` and `!`, as VS Code's `Repository.headLabel`
//! shows them.

use std::fs;

use gpui::TestAppContext;

use super::{branch_item, fixture, git, init, refresh, window};

#[gpui::test]
fn branch_item_marks_staged_and_working_changes_apart(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut vcx) = window(cx, &work);
    let cx = &mut vcx;
    let item = |cx: &mut _| branch_item(&workbench, cx);

    // Staged only.
    fs::write(work.join("a.txt"), "edited\n").unwrap();
    git(&work, &["add", "a.txt"]);
    refresh(&workbench, cx);
    assert_eq!(item(cx).as_deref(), Some("main+"));

    // Staged, then edited again.
    fs::write(work.join("a.txt"), "edited twice\n").unwrap();
    refresh(&workbench, cx);
    assert_eq!(item(cx).as_deref(), Some("main*+"));

    // An untracked file alone.
    git(&work, &["reset", "-q", "--hard"]);
    fs::write(work.join("new.txt"), "new\n").unwrap();
    refresh(&workbench, cx);
    assert_eq!(item(cx).as_deref(), Some("main*"));

    // Clean.
    fs::remove_file(work.join("new.txt")).unwrap();
    refresh(&workbench, cx);
    assert_eq!(item(cx).as_deref(), Some("main"));

    // A merge stopped on a conflict.
    fs::write(work.join("a.txt"), "main side\n").unwrap();
    git(&work, &["commit", "-q", "-am", "main side"]);
    let merge = std::process::Command::new("git")
        .args(["merge", "-q", "feat"])
        .current_dir(&work)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(!merge.status.success(), "the merge conflicts");
    refresh(&workbench, cx);
    assert_eq!(item(cx).as_deref(), Some("main!"));
}
