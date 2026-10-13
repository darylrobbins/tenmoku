//! A rebase pull signs as `commit.gpgSign` says and nothing else:
//! `branch.<name>.mergeOptions` is read by `git merge` only.
//!
//! The library's git calls inherit the process environment, so each test runs
//! again in a child process whose git sees no global or system config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{PullMode, RemoteError, Repo};

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
            "-c",
            "protocol.file.allow=always",
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

fn commit(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

fn rev(root: &Path, name: &str) -> String {
    git(root, &["rev-parse", name])
}

/// Set in the child process a test reruns itself in.
const ISOLATED: &str = "FE_GIT_PULL_REBASE_SIGNING_ISOLATED";

/// `true` in the child process, where the test's body runs. In the test
/// process itself, runs test `name` in a child whose git sees no global or
/// system config, fails if it fails, and returns `false`.
fn isolated(name: &str) -> bool {
    if std::env::var_os(ISOLATED).is_some() {
        return true;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(ISOLATED, "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for (var, value) in [
        ("GIT_AUTHOR_NAME", "t"),
        ("GIT_AUTHOR_EMAIL", "t@t"),
        ("GIT_COMMITTER_NAME", "t"),
        ("GIT_COMMITTER_EMAIL", "t@t"),
    ] {
        command.env(var, value);
    }
    for var in ["GIT_SSH", "GIT_SSH_COMMAND", "GIT_DIR", "GIT_WORK_TREE"] {
        command.env_remove(var);
    }
    let output = command.output().expect("rerun the test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{name} failed in isolation:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

/// A diverged clone `a`: `b` pushed a commit to the origin, `a` has its own.
struct Diverged {
    _dir: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
    ran: PathBuf,
}

/// `a`'s signer touches a sentinel file and fails.
#[cfg(unix)]
fn diverged() -> Diverged {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("seed");
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-q"]);
    commit(&seed, "f", "one\n");
    git(dir.path(), &["clone", "-q", "--bare", "seed", "origin.git"]);
    for clone in ["a", "b"] {
        git(dir.path(), &["clone", "-q", "origin.git", clone]);
    }
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let signer = dir.path().join("signer");
    let ran = dir.path().join("signer-ran");
    fs::write(
        &signer,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display()),
    )
    .unwrap();
    fs::set_permissions(&signer, fs::Permissions::from_mode(0o755)).unwrap();
    git(&a, &["config", "gpg.program", signer.to_str().unwrap()]);
    commit(&b, "g", "from b\n");
    git(&b, &["push", "-q"]);
    commit(&a, "h", "from a\n");
    Diverged {
        _dir: dir,
        a,
        b,
        ran,
    }
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn expect_signing_refused(result: Result<(), RemoteError>) {
    match result {
        Err(RemoteError::Failed { output }) => {
            assert!(output.contains("signing"), "{output}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// `--no-gpg-sign` in mergeOptions doesn't stop a rebase signing, so the pull
/// is refused before the rebase starts.
#[cfg(unix)]
#[test]
fn rebase_pull_with_no_gpg_sign_merge_options_is_refused_unchanged() {
    let name = "rebase_pull_with_no_gpg_sign_merge_options_is_refused_unchanged";
    if !isolated(name) {
        return;
    }
    let r = diverged();
    git(&r.a, &["config", "commit.gpgSign", "true"]);
    git(
        &r.a,
        &["config", "branch.main.mergeOptions", "--no-gpg-sign"],
    );
    let before = rev(&r.a, "HEAD");

    expect_signing_refused(open(&r.a).pull(PullMode::Rebase));

    assert_eq!(rev(&r.a, "HEAD"), before);
    assert_eq!(git(&r.a, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    for dir in ["rebase-merge", "rebase-apply"] {
        assert!(!r.a.join(".git").join(dir).exists(), "{dir} left behind");
    }
    assert_eq!(git(&r.a, &["status", "--porcelain"]), "");
    assert!(!r.ran.exists(), "the signer ran");
}

/// `-S` in mergeOptions doesn't make a rebase sign.
#[cfg(unix)]
#[test]
fn rebase_pull_ignores_merge_options_sign() {
    if !isolated("rebase_pull_ignores_merge_options_sign") {
        return;
    }
    let r = diverged();
    git(&r.a, &["config", "commit.gpgSign", "false"]);
    git(&r.a, &["config", "branch.main.mergeOptions", "-S"]);

    open(&r.a).pull(PullMode::Rebase).unwrap();

    assert_eq!(rev(&r.a, "HEAD^"), rev(&r.b, "HEAD"));
    assert_eq!(git(&r.a, &["log", "-1", "--format=%s"]), "from a");
    assert!(!r.ran.exists(), "the signer ran");
}

/// A merge does read mergeOptions.
#[cfg(unix)]
#[test]
fn merge_pull_with_merge_options_sign_is_refused() {
    if !isolated("merge_pull_with_merge_options_sign_is_refused") {
        return;
    }
    let r = diverged();
    git(&r.a, &["config", "commit.gpgSign", "false"]);
    git(&r.a, &["config", "branch.main.mergeOptions", "-S"]);
    let before = rev(&r.a, "HEAD");

    expect_signing_refused(open(&r.a).pull(PullMode::Merge));

    assert_eq!(rev(&r.a, "HEAD"), before);
    assert!(!r.ran.exists(), "the signer ran");
}
