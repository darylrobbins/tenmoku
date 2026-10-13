//! `status_split` lists an intent-to-add file as git status does: unstaged.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{ChangeKind, Repo, SplitStatus};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with one commit and a new intent-to-add file `new`.
fn repo_with_intent_to_add() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    fs::write(root.join("base.txt"), "base\n").unwrap();
    git(&root, &["add", "base.txt"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    fs::write(root.join("new"), "new\n").unwrap();
    git(&root, &["add", "-N", "new"]);
    (dir, root)
}

fn split(root: &Path) -> Vec<SplitStatus> {
    Repo::discover(root)
        .unwrap()
        .expect("a repository")
        .status_split()
        .unwrap()
}

fn entry(root: &Path, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> SplitStatus {
    SplitStatus {
        path: root.join("new"),
        staged,
        unstaged,
    }
}

#[test]
fn intent_to_add_file_is_unstaged_added() {
    let (_dir, root) = repo_with_intent_to_add();
    assert_eq!(split(&root), [entry(&root, None, Some(ChangeKind::Added))]);
}

#[test]
fn staged_new_file_is_staged_added() {
    let (_dir, root) = repo_with_intent_to_add();
    git(&root, &["add", "new"]);
    assert_eq!(split(&root), [entry(&root, Some(ChangeKind::Added), None)]);
}

#[test]
fn intent_to_add_file_deleted_from_disk_is_unstaged_deleted() {
    let (_dir, root) = repo_with_intent_to_add();
    fs::remove_file(root.join("new")).unwrap();
    assert_eq!(
        split(&root),
        [entry(&root, None, Some(ChangeKind::Deleted))]
    );
}
