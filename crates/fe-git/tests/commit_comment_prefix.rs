//! Committing when `core.commentChar` and `core.commentString` are both set.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{CommitError, Repo};

const ISOLATED_ENV: &str = "FE_GIT_COMMENT_PREFIX_ISOLATED";

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
fn isolated(name: &str, global: &str) -> bool {
    if std::env::var_os(ISOLATED_ENV).is_some() {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let global_path = dir.path().join("gitconfig");
    fs::write(&global_path, global).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(ISOLATED_ENV, "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &global_path)
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
    drop(dir);
    false
}

fn log_message(root: &Path) -> String {
    git(root, &["log", "-1", "--format=%B"])
}

#[test]
fn comment_char_set_after_comment_string_wins() {
    if !isolated("comment_char_set_after_comment_string_wins", "") {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentString", "#"]);
    git(&root, &["config", "--add", "core.commentChar", ";"]);
    let id = repo.commit("#42 fix crash\n", false).unwrap();
    assert_eq!(id, git(&root, &["rev-parse", "HEAD"]));
    assert_eq!(log_message(&root), "#42 fix crash");
}

#[test]
fn repo_comment_char_overrides_global_comment_string() {
    if !isolated(
        "repo_comment_char_overrides_global_comment_string",
        "[core]\n\tcommentString = \"#\"\n",
    ) {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentChar", ";"]);
    let id = repo.commit("#42 fix crash\n", false).unwrap();
    assert_eq!(id, git(&root, &["rev-parse", "HEAD"]));
    assert_eq!(log_message(&root), "#42 fix crash");
}

#[test]
fn uppercase_auto_comment_char_keeps_auto_line() {
    if !isolated("uppercase_auto_comment_char_keeps_auto_line", "") {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentChar", "AUTO"]);
    repo.commit("AUTO fix crash", false).unwrap();
    assert_eq!(log_message(&root), "AUTO fix crash");
}

#[test]
fn comment_string_set_after_comment_char_wins() {
    if !isolated("comment_string_set_after_comment_char_wins", "") {
        return;
    }
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.commentChar", ";"]);
    git(&root, &["config", "--add", "core.commentString", "#"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    let err = repo.commit("#42 only\n", false).unwrap_err();
    assert!(matches!(err, CommitError::EmptyMessage), "{err:?}");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    repo.commit("; note\n", false).unwrap();
    assert_eq!(log_message(&root), "; note");
}
