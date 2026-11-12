//! Ahead and behind with no remote: an upstream that's a local branch
//! (`branch.<name>.remote` of `.`), none at all, and the name
//! [`Repo::upstream_status`] gives each, beside [`Repo::upstream_counts`].

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::{Repo, Upstream};

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

fn commit(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn upstream(name: &str, ahead: u32, behind: u32) -> Option<Upstream> {
    Some(Upstream {
        name: name.to_owned(),
        ahead,
        behind,
    })
}

#[test]
fn a_local_upstream_counts_with_no_remote() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    commit(root, "a", "a1");
    git(root, &["switch", "-q", "--track", "-c", "feat", "main"]);
    let repo = open(root);
    assert_eq!(repo.upstream_counts().unwrap(), Some((0, 0)));
    assert_eq!(repo.upstream_status().unwrap(), upstream("main", 0, 0));

    commit(root, "b", "b1");
    commit(root, "c", "c1");
    git(root, &["switch", "-q", "main"]);
    commit(root, "d", "d1");
    git(root, &["switch", "-q", "feat"]);
    assert_eq!(repo.upstream_counts().unwrap(), Some((2, 1)));
    assert_eq!(repo.upstream_status().unwrap(), upstream("main", 2, 1));
}

#[test]
fn no_remote_and_no_upstream_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let repo = open(root);
    // Unborn.
    assert_eq!(repo.upstream_counts().unwrap(), None);
    assert_eq!(repo.upstream_status().unwrap(), None);

    commit(root, "a", "a1");
    assert_eq!(repo.upstream_counts().unwrap(), None);
    assert_eq!(repo.upstream_status().unwrap(), None);

    // Detached.
    git(root, &["switch", "-q", "--detach"]);
    assert_eq!(repo.upstream_counts().unwrap(), None);
    assert_eq!(repo.upstream_status().unwrap(), None);
}

#[test]
fn a_removed_remote_leaves_no_upstream() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    let origin = base.join("origin.git");
    let local = base.join("local");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q"]);
    commit(&local, "a", "a1");
    git(
        &local,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    commit(&local, "b", "b1");
    let repo = open(&local);
    assert_eq!(
        repo.upstream_status().unwrap(),
        upstream("origin/main", 1, 0)
    );

    git(&local, &["remote", "remove", "origin"]);
    assert_eq!(repo.upstream_counts().unwrap(), None);
    assert_eq!(repo.upstream_status().unwrap(), None);
}

#[test]
fn a_gone_or_unmapped_upstream_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    let origin = base.join("origin.git");
    let local = base.join("local");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q"]);
    commit(&local, "a", "a1");
    git(
        &local,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    let repo = open(&local);

    // Its ref gone, as after a pruning fetch.
    git(&local, &["update-ref", "-d", "refs/remotes/origin/main"]);
    assert_eq!(repo.upstream_status().unwrap(), None);

    // A fetch refspec that doesn't map it: `upstream_counts` calls that an
    // error, but there's still nothing to show.
    git(&local, &["fetch", "-q", "origin"]);
    assert_eq!(
        repo.upstream_status().unwrap(),
        upstream("origin/main", 0, 0)
    );
    let refspec = "+refs/heads/other:refs/remotes/origin/other";
    git(&local, &["config", "remote.origin.fetch", refspec]);
    assert_eq!(repo.upstream_status().unwrap(), None);
}
