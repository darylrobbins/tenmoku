// Its hooks are shell scripts made executable by mode.
#![cfg(unix)]

//! Committing the index, against temp repositories with hooks.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{CommitError, Repo};

const ISOLATED_ENV: &str = "FE_GIT_COMMIT_ISOLATED";

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

fn hook(root: &Path, name: &str, body: &str) {
    let path = root.join(".git/hooks").join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
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

#[test]
fn commits_and_amends() {
    if !isolated("commits_and_amends") {
        return;
    }
    let (_dir, root, repo) = init();
    let base = git(&root, &["rev-parse", "HEAD"]);
    assert!(repo.has_staged().unwrap());

    let id = repo.commit("# comment\nsecond\n\n# more\n", false).unwrap();
    assert_eq!(id, git(&root, &["rev-parse", "HEAD"]));
    assert_eq!(repo.head_id().as_deref(), Some(id.as_str()));
    assert_eq!(git(&root, &["log", "-1", "--format=%B"]), "second");
    assert_eq!(git(&root, &["rev-parse", "HEAD^"]), base);
    assert!(!repo.has_staged().unwrap());

    let amended = repo.commit("renamed", true).unwrap();
    assert_ne!(amended, id);
    assert_eq!(git(&root, &["log", "-1", "--format=%B"]), "renamed");
    assert_eq!(git(&root, &["rev-parse", "HEAD^"]), base);
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn hooks() {
    if !isolated("hooks") {
        return;
    }
    let (_dir, root, repo) = init();
    let head = git(&root, &["rev-parse", "HEAD"]);

    hook(&root, "pre-commit", "echo 'lint failed'\nexit 1");
    match repo.commit("msg", false) {
        Err(CommitError::HookFailed { hook, output }) => {
            assert_eq!(hook, "pre-commit");
            assert!(output.contains("lint failed"), "{output}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);

    let marker = root.join("ran");
    hook(
        &root,
        "pre-commit",
        &format!("touch '{}'", marker.display()),
    );
    hook(&root, "commit-msg", "echo 'bad message' >&2\nexit 1");
    match repo.commit("msg", false) {
        Err(CommitError::HookFailed { hook, output }) => {
            assert_eq!(hook, "commit-msg");
            assert!(output.contains("bad message"), "{output}");
        }
        other => panic!("{other:?}"),
    }
    assert!(marker.exists(), "the passing pre-commit hook ran");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);

    fs::remove_file(root.join(".git/hooks/commit-msg")).unwrap();
    fs::remove_file(&marker).unwrap();
    repo.commit("msg", false).unwrap();
    assert!(marker.exists());
}

#[test]
fn refusals() {
    if !isolated("refusals") {
        return;
    }
    let (_dir, root, repo) = init();
    let head = git(&root, &["rev-parse", "HEAD"]);
    let marker = root.join("ran");
    hook(
        &root,
        "pre-commit",
        &format!("touch '{}'", marker.display()),
    );

    assert!(matches!(
        repo.commit("# only\n  \n# comments\n", false),
        Err(CommitError::EmptyMessage)
    ));
    git(&root, &["reset", "-q"]);
    assert!(matches!(
        repo.commit("msg", false),
        Err(CommitError::NothingStaged)
    ));
    assert!(!marker.exists(), "git never ran");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
}

#[test]
fn large_message_and_verbose_hook_do_not_deadlock() {
    if !isolated("large_message_and_verbose_hook_do_not_deadlock") {
        return;
    }
    let (_dir, root, repo) = init();
    hook(
        &root,
        "pre-commit",
        "head -c 1000000 /dev/zero | tr '\\0' x",
    );
    let message = format!("{}\n", "m".repeat(1_000_000));
    repo.commit(&message, false).unwrap();
    assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn a_nested_git_failure_in_a_passing_hook_is_not_blamed() {
    if !isolated("a_nested_git_failure_in_a_passing_hook_is_not_blamed") {
        return;
    }
    let (_dir, root, repo) = init();
    hook(
        &root,
        "pre-commit",
        "git rev-parse --verify nope >/dev/null 2>&1\nexit 0",
    );
    hook(&root, "commit-msg", "exit 1");
    match repo.commit("msg", false) {
        Err(CommitError::HookFailed { hook, .. }) => assert_eq!(hook, "commit-msg"),
        other => panic!("{other:?}"),
    }
    // A failure that is no hook's: git itself refuses (a bad signing program).
    fs::remove_file(root.join(".git/hooks/commit-msg")).unwrap();
    git(&root, &["config", "commit.gpgsign", "true"]);
    git(&root, &["config", "gpg.program", "false"]);
    match repo.commit("msg", false) {
        Err(CommitError::Failed { .. }) => {}
        other => panic!("{other:?}"),
    }
}
