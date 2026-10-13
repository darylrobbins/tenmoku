//! How `switch` and `create_branch` classify git's refusals.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{Repo, SwitchError};

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn head(root: &Path) -> String {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// A repo on `main` with file `a`, and a branch `other` that adds file `d`.
fn init() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    fs::write(root.join("a"), "a").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "a"]);
    git(&root, &["checkout", "-q", "-b", "other"]);
    fs::write(root.join("d"), "d").unwrap();
    git(&root, &["add", "d"]);
    git(&root, &["commit", "-q", "-m", "d"]);
    git(&root, &["checkout", "-q", "main"]);
    (dir, root)
}

#[test]
fn switch_reports_untracked_files_in_a_directory_as_would_overwrite() {
    let (_dir, root) = init();
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    fs::create_dir(root.join("d")).unwrap();
    fs::write(root.join("d/u"), "untracked").unwrap();

    match repo.switch("other", false) {
        Err(SwitchError::WouldOverwrite { paths }) => {
            assert_eq!(paths, vec![repo.workdir().join("d")]);
        }
        other => panic!("expected WouldOverwrite, got {other:?}"),
    }
    assert_eq!(head(&root), "main");
    assert_eq!(fs::read_to_string(root.join("d/u")).unwrap(), "untracked");
}

#[test]
fn create_branch_rejects_names_git_would_expand() {
    let (_dir, root) = init();
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    // The previous branch is `other`, which exists.
    let branches = git(&root, &["branch", "--list"]);
    let result = repo.create_branch("@{-1}");
    assert!(
        matches!(result, Err(SwitchError::InvalidName)),
        "{result:?}"
    );
    assert_eq!(git(&root, &["branch", "--list"]), branches);

    // The previous branch was deleted: git would recreate it under its old name.
    git(&root, &["checkout", "-q", "-b", "gone"]);
    git(&root, &["checkout", "-q", "main"]);
    git(&root, &["branch", "-q", "-D", "gone"]);
    let branches = git(&root, &["branch", "--list"]);
    let result = repo.create_branch("@{-1}");
    assert!(
        matches!(result, Err(SwitchError::InvalidName)),
        "{result:?}"
    );
    assert_eq!(head(&root), "main");
    assert_eq!(git(&root, &["branch", "--list"]), branches);
}
