//! Which pulls would sign, and so are refused: `commit.gpgSign` for a rebase,
//! that and `branch.<name>.mergeOptions` (parsed as git does) for a merge.
//!
//! The library's git calls inherit the process environment, so each test runs
//! again in a child process whose git sees no global or system config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{PullMode, RemoteError, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = run_git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
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
        .expect("run git")
}

fn commit(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn rev(root: &Path, name: &str) -> String {
    git(root, &["rev-parse", name])
}

/// Set in the child process a test reruns itself in.
const ISOLATED: &str = "FE_GIT_PULL_SIGNING_TEST_ISOLATED";

/// `true` in the child process, where the test's body runs. In the test
/// process itself, runs test `name` in a child whose git sees no global or
/// system config, no proxy and no ssh command of the user's, fails if it
/// fails, and returns `false`.
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
    for var in [
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
    ] {
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

/// Clones `a` and `b` of a bare `origin.git` whose `main` has file f.
struct Remotes {
    _dir: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
}

fn remotes() -> Remotes {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("seed");
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-q"]);
    commit(&seed, "f", "one\n");
    git(dir.path(), &["clone", "-q", "--bare", "seed", "origin.git"]);
    for clone in ["a", "b"] {
        git(dir.path(), &["clone", "-q", "origin.git", clone]);
    }
    Remotes {
        a: dir.path().join("a"),
        b: dir.path().join("b"),
        _dir: dir,
    }
}

/// Makes A and B diverge: each adds a file of its own, B pushes.
fn diverge(r: &Remotes, n: usize) {
    git(&r.a, &["fetch", "-q"]);
    git(&r.a, &["reset", "-q", "--hard", "origin/main"]);
    git(&r.b, &["pull", "-q", "--ff-only"]);
    commit(&r.a, &format!("a{n}"), "from a\n");
    commit(&r.b, &format!("b{n}"), "from b\n");
    git(&r.b, &["push", "-q"]);
}

fn assert_refused(r: &Remotes, result: Result<(), RemoteError>, head: &str, what: &str) {
    match result {
        Err(RemoteError::Failed { output }) => {
            assert!(output.contains("signing"), "{what}: {output}");
        }
        other => panic!("{what}: expected Failed, got {other:?}"),
    }
    assert_eq!(rev(&r.a, "HEAD"), head, "{what}");
    assert_eq!(git(&r.a, &["status", "--porcelain"]), "", "{what}");
    let git_dir = r.a.join(".git");
    for state in ["MERGE_HEAD", "rebase-merge", "rebase-apply"] {
        assert!(!git_dir.join(state).exists(), "{what}: {state}");
    }
}

fn set(r: &Remotes, gpg_sign: &str, merge_options: &str) {
    git(&r.a, &["config", "commit.gpgSign", gpg_sign]);
    git(&r.a, &["config", "branch.main.mergeOptions", merge_options]);
}

#[test]
fn rebase_ignores_merge_options() {
    if !isolated("rebase_ignores_merge_options") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);

    diverge(&r, 1);
    set(&r, "true", "--no-gpg-sign");
    let head = rev(&r.a, "HEAD");
    assert_refused(
        &r,
        a.pull(PullMode::Rebase),
        &head,
        "gpgSign, --no-gpg-sign",
    );

    diverge(&r, 2);
    set(&r, "false", "-S");
    a.pull(PullMode::Rebase).unwrap();
    assert_eq!(rev(&r.a, "HEAD^"), rev(&r.b, "HEAD"));
    assert_eq!(git(&r.a, &["status", "--porcelain"]), "");
}

#[test]
fn merge_options_parse_like_git() {
    if !isolated("merge_options_parse_like_git") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let mut n = 0;

    for options in [
        "'-S'",
        "\"--gpg-sign\"",
        "--gpg",
        "--gpg-sign=key",
        "-nS",
        "--no-ff '-S",
    ] {
        n += 1;
        diverge(&r, n);
        set(&r, "false", options);
        let head = rev(&r.a, "HEAD");
        assert_refused(&r, a.pull(PullMode::Merge), &head, options);
    }

    for options in ["--no-gpg", "\"--no-gpg-sign\"", "-S --no-gpg-sign"] {
        n += 1;
        diverge(&r, n);
        set(&r, "true", options);
        a.pull(PullMode::Merge).expect(options);
        assert_eq!(rev(&r.a, "HEAD^2"), rev(&r.b, "HEAD"), "{options}");
    }
}

/// Option arguments and everything after `--` are not options to git's
/// `merge`, so they can't turn signing off.
#[test]
fn merge_option_values_and_dashdash_like_git() {
    if !isolated("merge_option_values_and_dashdash_like_git") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let mut n = 0;

    for options in [
        "-S -m --no-gpg",
        "-S -- --no-gpg",
        "-S --message --no-gpg-sign",
        "-S -nm --no-gpg-sign",
    ] {
        n += 1;
        diverge(&r, n);
        set(&r, "false", options);
        let head = rev(&r.a, "HEAD");
        assert_refused(&r, a.pull(PullMode::Merge), &head, options);
    }

    // `-mS` is the message `S`.
    for (gpg_sign, options) in [("false", "-mS"), ("true", "-m msg --no-gpg-sign")] {
        n += 1;
        diverge(&r, n);
        set(&r, gpg_sign, options);
        a.pull(PullMode::Merge).expect(options);
        assert_eq!(rev(&r.a, "HEAD^2"), rev(&r.b, "HEAD"), "{options}");
    }
}

/// Git's tokenizer splits on space, tab, LF and CR only, so other Unicode
/// whitespace stays inside a word, a signing key for instance.
#[test]
fn merge_options_split_on_git_whitespace() {
    if !isolated("merge_options_split_on_git_whitespace") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let mut n = 0;

    for options in [
        "--gpg-sign=key\u{a0}--no-gpg-sign",
        "--gpg-sign=key\u{b}--no-gpg-sign",
    ] {
        n += 1;
        diverge(&r, n);
        set(&r, "false", options);
        let head = rev(&r.a, "HEAD");
        assert_refused(&r, a.pull(PullMode::Merge), &head, options);
    }

    n += 1;
    diverge(&r, n);
    let options = "--gpg-sign=key\t--no-gpg-sign";
    set(&r, "true", options);
    a.pull(PullMode::Merge).expect(options);
    assert_eq!(rev(&r.a, "HEAD^2"), rev(&r.b, "HEAD"), "{options}");
}
