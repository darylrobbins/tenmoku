//! Typed branch names, resolved by their full configured remote name:
//! `team/origin/feat` is `feat` on `team/origin`, though a local branch
//! `origin/feat` exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::Repo;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn assert_on_feat(root: &Path) {
    assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/feat");
    assert_eq!(
        git(
            root,
            &["rev-parse", "--symbolic-full-name", "feat@{upstream}"]
        ),
        "refs/remotes/team/origin/feat"
    );
}

/// A repo on `main` with `a.txt`; remote `team/origin` with a
/// remote-tracking `feat` that changes it; a local `origin/feat` at main.
fn init() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    fs::write(root.join("a.txt"), "main\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "main"]);
    git(&root, &["switch", "-q", "-c", "tmp"]);
    fs::write(root.join("a.txt"), "feat\n").unwrap();
    git(&root, &["commit", "-q", "-am", "feat"]);
    let feat = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "-q", "main"]);
    git(&root, &["branch", "-q", "-D", "tmp"]);
    let url = dir.path().join("team-origin.git");
    git(
        &root,
        &["remote", "add", "team/origin", url.to_str().unwrap()],
    );
    git(
        &root,
        &["update-ref", "refs/remotes/team/origin/feat", &feat],
    );
    git(&root, &["branch", "-q", "origin/feat"]);
    (dir, root)
}

#[test]
fn typed_name_under_a_remote_with_a_slash_tracks_that_remote() {
    let (_dir, root) = init();
    open(&root).switch("team/origin/feat", false).unwrap();
    assert_on_feat(&root);
}

#[test]
fn stash_and_switch_by_typed_name() {
    let (_dir, root) = init();
    fs::write(root.join("a.txt"), "mine\n").unwrap();
    open(&root).stash_and_switch("team/origin/feat").unwrap();
    assert_on_feat(&root);
    assert_eq!(git(&root, &["stash", "list"]).lines().count(), 1);
}
