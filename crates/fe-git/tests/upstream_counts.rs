//! Ahead and behind counts against an upstream, with a local bare `origin`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::Repo;

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

struct Setup {
    _dir: tempfile::TempDir,
    origin: PathBuf,
    local: PathBuf,
    other: PathBuf,
}

/// `origin` (bare) holds main with 2 commits. `local` is a clone; `other` is
/// a second clone that pushes one more commit. Then `local` makes 2 commits of
/// its own and fetches: main is 2 ahead of and 1 behind origin/main.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let origin = base.join("origin.git");
    let local = base.join("local");
    let other = base.join("other");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    git(&base, &["clone", "-q", origin.to_str().unwrap(), "local"]);
    commit(&local, "a", "a1");
    commit(&local, "b", "b1");
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    git(&base, &["clone", "-q", origin.to_str().unwrap(), "other"]);
    commit(&other, "c", "c1");
    git(&other, &["push", "-q", "origin", "main"]);
    commit(&local, "d", "d1");
    commit(&local, "e", "e1");
    git(&local, &["fetch", "-q"]);
    Setup {
        _dir: dir,
        origin,
        local,
        other,
    }
}

#[test]
fn branch_counts() {
    let s = setup();
    git(&s.local, &["branch", "loose"]);
    let branches = open(&s.local).branches().unwrap();
    let main = branches.iter().find(|b| b.name == "main").unwrap();
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!((main.ahead, main.behind), (Some(2), Some(1)));
    assert!(!main.upstream_gone);
    let loose = branches.iter().find(|b| b.name == "loose").unwrap();
    assert_eq!(
        (loose.ahead, loose.behind, loose.upstream_gone),
        (None, None, false)
    );
    let remotes: Vec<_> = branches.iter().filter(|b| b.remote.is_some()).collect();
    assert!(!remotes.is_empty());
    for remote in remotes {
        assert_eq!(
            (remote.ahead, remote.behind, remote.upstream_gone),
            (None, None, false)
        );
    }
}

#[test]
fn gone_upstream() {
    let s = setup();
    git(&s.local, &["branch", "topic", "main"]);
    git(&s.local, &["push", "-q", "-u", "origin", "topic"]);
    git(&s.origin, &["branch", "-D", "topic"]);
    git(&s.local, &["fetch", "-q", "--prune"]);
    let branches = open(&s.local).branches().unwrap();
    let topic = branches.iter().find(|b| b.name == "topic").unwrap();
    assert_eq!(
        (topic.ahead, topic.behind, topic.upstream_gone),
        (None, None, true)
    );
    let main = branches.iter().find(|b| b.name == "main").unwrap();
    assert!(!main.upstream_gone);
}

#[test]
fn head_counts() {
    let s = setup();
    let repo = open(&s.local);
    assert_eq!(repo.upstream_counts().unwrap(), Some((2, 1)));

    git(&s.local, &["reset", "-q", "--hard", "origin/main"]);
    assert_eq!(repo.upstream_counts().unwrap(), Some((0, 0)));

    // No upstream.
    git(&s.local, &["checkout", "-q", "-b", "loose"]);
    assert_eq!(repo.upstream_counts().unwrap(), None);

    // Upstream gone.
    git(&s.local, &["push", "-q", "-u", "origin", "loose"]);
    git(&s.other, &["fetch", "-q"]);
    git(&s.origin, &["branch", "-D", "loose"]);
    git(&s.local, &["fetch", "-q", "--prune"]);
    assert_eq!(repo.upstream_counts().unwrap(), None);

    // Detached.
    git(&s.local, &["checkout", "-q", "--detach"]);
    assert_eq!(repo.upstream_counts().unwrap(), None);

    // Unborn.
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    assert_eq!(open(dir.path()).upstream_counts().unwrap(), None);
}
