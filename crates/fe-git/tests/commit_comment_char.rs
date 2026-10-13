//! Committing under a custom `core.commentChar`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{CommitError, Repo};

const ISOLATED_ENV: &str = "FE_GIT_COMMENT_CHAR_ISOLATED";

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

/// A repository with one commit and one staged change.
fn init() -> (tempfile::TempDir, PathBuf, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    fs::write(root.join("a.txt"), "one\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.txt"), "two\n").unwrap();
    git(&root, &["add", "."]);
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    (dir, root, repo)
}

/// Reruns the calling test in a child with the developer's git config shut
/// out. True in the child, which runs the test body.
fn isolated(name: &str) -> bool {
    if std::env::var_os(ISOLATED_ENV).is_some() {
        return true;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(ISOLATED_ENV, "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

fn log_message(root: &Path) -> String {
    git(root, &["log", "-1", "--format=%B"])
}

#[test]
fn hash_line_commits_when_comment_char_is_semicolon() {
    if !isolated("hash_line_commits_when_comment_char_is_semicolon") {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentChar", ";"]);
    let id = repo.commit("#42 fix crash\n#7 too\n", false).unwrap();
    assert_eq!(id, git(&root, &["rev-parse", "HEAD"]));
    assert_eq!(log_message(&root), "#42 fix crash\n#7 too");
}

#[test]
fn hash_line_commits_when_comment_char_is_auto() {
    if !isolated("hash_line_commits_when_comment_char_is_auto") {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentChar", "auto"]);
    repo.commit("#42 fix crash", false).unwrap();
    assert_eq!(log_message(&root), "#42 fix crash");
}

#[test]
fn only_custom_comment_lines_is_empty_message() {
    if !isolated("only_custom_comment_lines_is_empty_message") {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentChar", ";"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    let err = repo.commit("; only\n  \n; comments\n", false).unwrap_err();
    assert!(matches!(err, CommitError::EmptyMessage), "{err:?}");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
}
