//! Ahead and behind counts against an upstream, with a local bare `origin`.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::{GitError, Repo};

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

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

#[test]
fn unmapped_upstream_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    let origin = base.join("origin.git");
    let local = base.join("local");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    git(base, &["clone", "-q", origin.to_str().unwrap(), "local"]);
    commit(&local, "a", "a1");
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    let repo = open(&local);
    assert_eq!(repo.upstream_counts().unwrap(), Some((0, 0)));

    let check = |repo: &Repo| match repo.upstream_counts() {
        Err(GitError::Command { stderr, .. }) => assert!(
            stderr.contains("not stored as a remote-tracking branch"),
            "stderr: {stderr}"
        ),
        other => panic!("expected a command error, got {other:?}"),
    };

    git(
        &local,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/other:refs/remotes/origin/other",
        ],
    );
    check(&repo);

    git(
        &local,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    git(&local, &["config", "branch.main.remote", "nosuch"]);
    check(&repo);
}
