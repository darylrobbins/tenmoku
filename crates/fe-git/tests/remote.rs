//! Fetching, pulling and pushing between two clones of a local bare repository.
//!
//! The library's git calls inherit the process environment, so each test runs
//! again in a child process whose git sees no global or system config.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fe_git::{GitError, PullMode, RemoteError, Repo};

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

fn has_ref(root: &Path, name: &str) -> bool {
    run_git(root, &["show-ref", "--verify", "--quiet", name])
        .status
        .success()
}

/// Set in the child process a test reruns itself in.
const ISOLATED: &str = "FE_GIT_REMOTE_TEST_ISOLATED";

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
    origin: PathBuf,
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
    let origin = dir.path().join("origin.git");
    for clone in ["a", "b"] {
        git(dir.path(), &["clone", "-q", "origin.git", clone]);
    }
    Remotes {
        a: dir.path().join("a"),
        b: dir.path().join("b"),
        origin,
        _dir: dir,
    }
}

#[test]
fn fetch_and_pull() {
    if !isolated("fetch_and_pull") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let a_head = rev(&r.a, "HEAD");

    commit(&r.b, "g", "from b\n");
    git(&r.b, &["push", "-q"]);
    let b_head = rev(&r.b, "HEAD");

    a.fetch(false).unwrap();
    assert_eq!(rev(&r.a, "origin/main"), b_head);
    assert_eq!(rev(&r.a, "HEAD"), a_head);
    assert!(!r.a.join(".git/FETCH_HEAD").exists());

    a.pull(PullMode::FastForwardOnly).unwrap();
    assert_eq!(rev(&r.a, "HEAD"), b_head);
    assert_eq!(fs::read_to_string(r.a.join("g")).unwrap(), "from b\n");

    // A branch B pushes and then deletes on origin.
    git(&r.b, &["push", "-q", "origin", "HEAD:refs/heads/gone"]);
    a.fetch(false).unwrap();
    assert!(has_ref(&r.a, "refs/remotes/origin/gone"));
    git(&r.b, &["push", "-q", "origin", "--delete", "gone"]);
    a.fetch(false).unwrap();
    assert!(has_ref(&r.a, "refs/remotes/origin/gone"));
    a.fetch(true).unwrap();
    assert!(!has_ref(&r.a, "refs/remotes/origin/gone"));
    assert!(has_ref(&r.a, "refs/remotes/origin/main"));

    // Detached, A fetches origin.
    git(&r.a, &["checkout", "-q", "--detach"]);
    commit(&r.b, "g", "again from b\n");
    git(&r.b, &["push", "-q"]);
    a.fetch(false).unwrap();
    assert_eq!(rev(&r.a, "origin/main"), rev(&r.b, "HEAD"));
}

#[test]
fn pull_conflicts() {
    if !isolated("pull_conflicts") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    commit(&r.a, "f", "from a\n");
    commit(&r.b, "f", "from b\n");
    git(&r.b, &["push", "-q"]);
    let a_head = rev(&r.a, "HEAD");

    match a.pull(PullMode::Merge) {
        Err(RemoteError::Conflicts { paths }) => {
            assert_eq!(paths, vec![a.workdir().join("f")]);
            assert!(paths[0].is_absolute());
        }
        other => panic!("expected Conflicts, got {other:?}"),
    }
    assert_eq!(rev(&r.a, "MERGE_HEAD"), rev(&r.b, "HEAD"));

    // Back to the same start: A's commit on top of the old main.
    git(&r.a, &["merge", "--abort"]);
    assert_eq!(rev(&r.a, "HEAD"), a_head);

    match a.pull(PullMode::FastForwardOnly) {
        Err(RemoteError::NotFastForward) => {}
        other => panic!("expected NotFastForward, got {other:?}"),
    }
    assert_eq!(rev(&r.a, "HEAD"), a_head);
    assert_eq!(fs::read_to_string(r.a.join("f")).unwrap(), "from a\n");
    assert_eq!(git(&r.a, &["status", "--porcelain"]), "");
}

#[test]
fn push() {
    if !isolated("push") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    commit(&r.b, "g", "from b\n");
    git(&r.b, &["push", "-q"]);
    commit(&r.a, "h", "from a\n");

    match a.push(false) {
        Err(RemoteError::Rejected { output }) => {
            assert!(output.contains("[rejected]"), "{output}");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert_eq!(rev(&r.origin, "main"), rev(&r.b, "HEAD"));

    a.pull(PullMode::Rebase).unwrap();
    assert_eq!(rev(&r.a, "HEAD~"), rev(&r.b, "HEAD"));
    a.push(false).unwrap();
    assert_eq!(rev(&r.a, "origin/main"), rev(&r.a, "HEAD"));
    assert_eq!(rev(&r.origin, "main"), rev(&r.a, "HEAD"));

    // B pushes again; A's view of origin/main is now stale, so its lease is.
    git(&r.b, &["pull", "-q", "--ff-only"]);
    commit(&r.b, "g", "from b again\n");
    git(&r.b, &["push", "-q"]);
    git(&r.a, &["commit", "-q", "--amend", "-m", "amended"]);
    match a.push(true) {
        Err(RemoteError::Rejected { output }) => {
            assert!(output.contains("[rejected]"), "{output}");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert_eq!(rev(&r.origin, "main"), rev(&r.b, "HEAD"));

    // Once fetched, the lease holds and A's amend replaces B's commit.
    a.fetch(false).unwrap();
    a.push(true).unwrap();
    assert_eq!(rev(&r.origin, "main"), rev(&r.a, "HEAD"));
}

#[test]
fn publish_and_errors() {
    if !isolated("publish_and_errors") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    git(&r.a, &["switch", "-q", "-c", "feat"]);
    commit(&r.a, "h", "feature\n");

    match a.push(false) {
        Err(RemoteError::NoUpstream) => {}
        other => panic!("expected NoUpstream, got {other:?}"),
    }
    match a.pull(PullMode::Merge) {
        Err(RemoteError::NoUpstream) => {}
        other => panic!("expected NoUpstream, got {other:?}"),
    }
    a.publish("origin").unwrap();
    assert_eq!(rev(&r.origin, "feat"), rev(&r.a, "HEAD"));
    assert_eq!(
        git(&r.a, &["rev-parse", "--abbrev-ref", "feat@{upstream}"]),
        "origin/feat"
    );
    commit(&r.a, "h", "more\n");
    a.push(false).unwrap();
    assert_eq!(rev(&r.origin, "feat"), rev(&r.a, "HEAD"));

    git(&r.a, &["checkout", "-q", "--detach"]);
    match a.publish("origin") {
        Err(RemoteError::Detached) => {}
        other => panic!("expected Detached, got {other:?}"),
    }

    let lone = tempfile::tempdir().unwrap();
    git(lone.path(), &["init", "-q"]);
    commit(lone.path(), "f", "one\n");
    match open(lone.path()).fetch(false) {
        Err(RemoteError::NoRemote) => {}
        other => panic!("expected NoRemote, got {other:?}"),
    }

    // A config git can't read is git's error, not a missing remote.
    let broken = open(lone.path());
    fs::write(lone.path().join(".git/config"), "[core\n").unwrap();
    match broken.fetch(false) {
        Err(RemoteError::Git(GitError::Command { stderr, .. })) => {
            assert!(stderr.contains("config"), "{stderr}");
        }
        other => panic!("expected a git error, got {other:?}"),
    }

    // Nothing listens on these ports.
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    for url in [
        format!("ssh://git@127.0.0.1:{port}/x.git"),
        format!("http://127.0.0.1:{port}/x.git"),
    ] {
        match fetch_from(&url) {
            Err(RemoteError::Failed { output } | RemoteError::Auth { output }) => {
                assert!(!output.is_empty(), "{url}");
            }
            other => panic!("{url}: expected Failed, got {other:?}"),
        }
    }

    // A server that wants credentials: with no helper and no prompt, git gives up.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in server.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\n\
                  Content-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    match fetch_from(&format!("http://127.0.0.1:{port}/x.git")) {
        Err(RemoteError::Auth { output }) => {
            assert!(output.contains("could not read Username"), "{output}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
}

/// Fetches into a fresh repository whose `origin` is `url`, failing the test
/// if that takes 10 s or more. Git's stdin is null, so a prompt can't wait on
/// it.
fn fetch_from(url: &str) -> Result<(), RemoteError> {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["remote", "add", "origin", url]);
    let repo = open(dir.path());
    let (send, receive) = mpsc::channel();
    let start = Instant::now();
    std::thread::spawn(move || {
        let _ = send.send(repo.fetch(false));
    });
    let result = receive
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("fetch from {url} still running after 10 s"));
    assert!(start.elapsed() < Duration::from_secs(10));
    result
}

#[test]
fn pull_never_autostashes() {
    if !isolated("pull_never_autostashes") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    git(&r.a, &["config", "merge.autoStash", "true"]);
    git(&r.a, &["config", "rebase.autoStash", "true"]);
    commit(&r.b, "f", "from b\n");
    git(&r.b, &["push", "-q"]);
    fs::write(r.a.join("f"), "local\n").unwrap();
    let a_head = rev(&r.a, "HEAD");

    for mode in [PullMode::Merge, PullMode::Rebase] {
        match a.pull(mode) {
            Err(RemoteError::Failed { .. }) => {}
            other => panic!("{mode:?}: expected Failed, got {other:?}"),
        }
        assert_eq!(rev(&r.a, "HEAD"), a_head);
        assert_eq!(fs::read_to_string(r.a.join("f")).unwrap(), "local\n");
        assert_eq!(git(&r.a, &["stash", "list"]), "");
    }
}

/// A signer at `dir/signer` that records it ran (in `dir/signer-ran`, the
/// path returned) and fails.
#[cfg(unix)]
fn sentinel_signer(dir: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let signer = dir.join("signer");
    let ran = dir.join("signer-ran");
    fs::write(
        &signer,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display()),
    )
    .unwrap();
    fs::set_permissions(&signer, fs::Permissions::from_mode(0o755)).unwrap();
    (signer, ran)
}

fn expect_signing_refused(result: Result<(), RemoteError>, what: &str) {
    match result {
        Err(RemoteError::Failed { output }) => {
            assert!(output.contains("signing"), "{what}: {output}");
        }
        other => panic!("{what}: expected Failed, got {other:?}"),
    }
}

/// With signing on, nothing that would sign runs.
#[cfg(unix)]
#[test]
fn signing_is_refused() {
    if !isolated("signing_is_refused") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let (signer, ran) = sentinel_signer(r.a.parent().unwrap());
    git(&r.a, &["config", "gpg.program", signer.to_str().unwrap()]);
    git(&r.a, &["config", "commit.gpgSign", "true"]);

    // A fast-forward makes no commit, so it goes ahead.
    commit(&r.b, "g", "from b\n");
    git(&r.b, &["push", "-q"]);
    a.pull(PullMode::Merge).unwrap();
    assert_eq!(rev(&r.a, "HEAD"), rev(&r.b, "HEAD"));

    commit(&r.a, "h", "from a\n");
    commit(&r.b, "g", "again from b\n");
    git(&r.b, &["push", "-q"]);
    let a_head = rev(&r.a, "HEAD");
    let signing_merges = [
        ("commit.gpgSign", vec![]),
        (
            "mergeOptions -S",
            vec![
                ("commit.gpgSign", "false"),
                ("branch.main.mergeOptions", "-S"),
            ],
        ),
        (
            "mergeOptions --gpg-sign",
            vec![("branch.main.mergeOptions", "--no-ff --gpg-sign=key")],
        ),
    ];
    for (what, config) in signing_merges {
        for (key, value) in config {
            git(&r.a, &["config", key, value]);
        }
        // A rebase ignores mergeOptions: only commit.gpgSign makes it sign.
        let modes: &[PullMode] = if what == "commit.gpgSign" {
            &[PullMode::Merge, PullMode::Rebase]
        } else {
            &[PullMode::Merge]
        };
        for &mode in modes {
            expect_signing_refused(a.pull(mode), &format!("{what} {mode:?}"));
            assert_eq!(rev(&r.a, "HEAD"), a_head);
            assert_eq!(git(&r.a, &["status", "--porcelain"]), "");
        }
    }
    // mergeOptions can turn commit.gpgSign off again.
    git(&r.a, &["config", "commit.gpgSign", "true"]);
    git(
        &r.a,
        &["config", "branch.main.mergeOptions", "--no-gpg-sign"],
    );
    a.pull(PullMode::Merge).unwrap();
    assert_eq!(rev(&r.a, "HEAD^2"), rev(&r.b, "HEAD"));

    // push.gpgSign true, however git spells it.
    git(&r.a, &["reset", "-q", "--hard", "origin/main"]);
    commit(&r.a, "h", "from a\n");
    git(&r.a, &["switch", "-q", "-c", "feat"]);
    git(&r.a, &["switch", "-q", "main"]);
    for value in ["true", "On", "2", "-1", ""] {
        let _ = run_git(&r.a, &["config", "--unset-all", "push.gpgSign"]);
        if value.is_empty() {
            // A key with no value at all, which git reads as true.
            let config = r.a.join(".git/config");
            let mut text = fs::read_to_string(&config).unwrap();
            text.push_str("[push]\n\tgpgSign\n");
            fs::write(&config, text).unwrap();
        } else {
            git(&r.a, &["config", "push.gpgSign", value]);
        }
        expect_signing_refused(a.push(false), &format!("push.gpgSign={value:?}"));
        git(&r.a, &["switch", "-q", "feat"]);
        expect_signing_refused(a.publish("origin"), &format!("push.gpgSign={value:?}"));
        git(&r.a, &["switch", "-q", "main"]);
    }
    assert_eq!(rev(&r.origin, "main"), rev(&r.b, "HEAD"));
    assert!(!has_ref(&r.origin, "refs/heads/feat"));

    // if-asked, even to a server that asks for a signed push, pushes unsigned.
    let _ = run_git(&r.a, &["config", "--unset-all", "push.gpgSign"]);
    git(&r.a, &["config", "push.gpgSign", "if-asked"]);
    git(&r.origin, &["config", "receive.certNonceSeed", "seed"]);
    git(&r.a, &["switch", "-q", "feat"]);
    a.publish("origin").unwrap();
    assert_eq!(rev(&r.origin, "feat"), rev(&r.a, "HEAD"));
    assert!(!ran.exists());
}

/// Submodules: a pull leaves them alone (no update, so no autostash or
/// signed rebase in one), and a push that recurses into one can't reach its
/// signer.
#[cfg(unix)]
#[test]
fn submodules_never_sign_or_stash() {
    if !isolated("submodules_never_sign_or_stash") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let dir = r.a.parent().unwrap();
    let (signer, ran) = sentinel_signer(dir);

    // A bare sub.git that asks for signed pushes, as submodule `sub` of main.
    let sub_seed = dir.join("sub-seed");
    fs::create_dir(&sub_seed).unwrap();
    git(&sub_seed, &["init", "-q"]);
    commit(&sub_seed, "s", "s1\n");
    git(dir, &["clone", "-q", "--bare", "sub-seed", "sub.git"]);
    git(
        &dir.join("sub.git"),
        &["config", "receive.certNonceSeed", "seed"],
    );
    let sub_url = dir.join("sub.git");
    git(
        &r.b,
        &["submodule", "add", "-q", sub_url.to_str().unwrap(), "sub"],
    );
    git(&r.b, &["commit", "-q", "-m", "add sub"]);
    git(&r.b, &["push", "-q"]);
    git(&r.a, &["pull", "-q", "--ff-only"]);
    git(&r.a, &["submodule", "update", "-q", "--init"]);

    // A's submodule has a local commit and a local change, rebases with
    // autostash and signs; B moves the submodule on.
    let a_sub = r.a.join("sub");
    for (key, value) in [
        ("rebase.autoStash", "true"),
        ("commit.gpgSign", "true"),
        ("push.gpgSign", "true"),
        ("gpg.program", signer.to_str().unwrap()),
        ("protocol.file.allow", "always"),
    ] {
        git(&a_sub, &["config", key, value]);
    }
    commit(&a_sub, "t", "from a\n");
    fs::write(a_sub.join("s"), "local\n").unwrap();
    let a_sub_head = rev(&a_sub, "HEAD");
    git(&r.a, &["config", "submodule.recurse", "true"]);
    git(&r.a, &["config", "pull.rebase", "true"]);

    let b_sub = r.b.join("sub");
    commit(&b_sub, "s", "from b\n");
    git(&b_sub, &["push", "-q", "origin", "HEAD:main"]);
    git(&r.b, &["commit", "-q", "-am", "move sub"]);
    git(&r.b, &["push", "-q"]);

    a.pull(PullMode::Rebase).unwrap();
    assert_eq!(rev(&r.a, "HEAD"), rev(&r.b, "HEAD"));
    assert_eq!(rev(&a_sub, "HEAD"), a_sub_head);
    assert_eq!(fs::read_to_string(a_sub.join("s")).unwrap(), "local\n");
    assert_eq!(git(&a_sub, &["stash", "list"]), "");

    // Pushing main with A's submodule commit pushes the submodule first (on a
    // branch of the same name, ahead of its remote), and that push would sign.
    git(&a_sub, &["checkout", "-q", "--", "s"]);
    git(&a_sub, &["fetch", "-q", "origin"]);
    git(&a_sub, &["switch", "-q", "-C", "main"]);
    git(&a_sub, &["rebase", "-q", "origin/main"]);
    git(&r.a, &["add", "sub"]);
    git(&r.a, &["commit", "-q", "-m", "sub at a"]);
    git(&r.a, &["config", "push.recurseSubmodules", "on-demand"]);
    let origin_sub = rev(&sub_url, "main");
    assert!(a.push(false).is_err());
    git(&r.a, &["switch", "-q", "-c", "feat"]);
    git(&a_sub, &["switch", "-q", "-c", "feat"]);
    assert!(a.publish("origin").is_err());
    assert_eq!(rev(&sub_url, "main"), origin_sub);
    assert!(!has_ref(&sub_url, "refs/heads/feat"));
    assert!(!ran.exists());

    // push.recurseSubmodules=check stays in force: the unpublished submodule
    // commit stops the push, and neither the superproject nor the submodule
    // is pushed.
    git(&r.a, &["config", "push.recurseSubmodules", "check"]);
    let origin_main = rev(&r.origin, "main");
    assert!(a.publish("origin").is_err());
    assert!(!has_ref(&r.origin, "refs/heads/feat"));
    git(&r.a, &["switch", "-q", "main"]);
    assert!(a.push(false).is_err());
    assert_eq!(rev(&r.origin, "main"), origin_main);
    assert_eq!(rev(&sub_url, "main"), origin_sub);
    assert!(!ran.exists());
}

/// A conflict in a file whose name isn't UTF-8 is reported by its real name.
#[cfg(target_os = "linux")]
#[test]
fn conflict_path_not_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    if !isolated("conflict_path_not_utf8") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let name = OsStr::from_bytes(b"caf\xe9");
    fs::write(r.a.join(name), "base\n").unwrap();
    git(&r.a, &["add", "."]);
    git(&r.a, &["commit", "-q", "-m", "base"]);
    git(&r.a, &["push", "-q"]);
    git(&r.b, &["pull", "-q", "--ff-only"]);
    for (root, text) in [(&r.a, "from a\n"), (&r.b, "from b\n")] {
        fs::write(root.join(name), text).unwrap();
        git(root, &["commit", "-q", "-am", text]);
    }
    git(&r.b, &["push", "-q"]);

    match a.pull(PullMode::Merge) {
        Err(RemoteError::Conflicts { paths }) => {
            assert_eq!(paths, vec![a.workdir().join(name)]);
        }
        other => panic!("expected Conflicts, got {other:?}"),
    }
}
