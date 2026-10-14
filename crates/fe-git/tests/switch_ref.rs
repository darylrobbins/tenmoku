//! Switching to a branch named by the kind of ref it is ([`BranchRef`]),
//! against temp repositories: a remote-tracking branch whose short name a
//! local branch also has, and remotes whose names hold a slash.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{BranchRef, Repo, SwitchError};

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

/// HEAD's full ref name.
fn head(root: &Path) -> String {
    git(root, &["symbolic-ref", "HEAD"])
}

fn upstream(root: &Path, branch: &str) -> String {
    let config = |key: &str| git(root, &["config", &format!("branch.{branch}.{key}")]);
    format!("{} {}", config("remote"), config("merge"))
}

fn local(name: &str) -> BranchRef {
    BranchRef::Local(name.to_owned())
}

fn remote(remote: &str, branch: &str) -> BranchRef {
    BranchRef::Remote {
        remote: remote.to_owned(),
        branch: branch.to_owned(),
    }
}

/// A repo on `main` with `a.txt`; remotes `origin` and `team/origin`, each
/// with a remote-tracking `feat` that changes `a.txt`; and a local branch
/// named `origin/feat`, at main. Identity is set locally so `git stash`
/// works from the library.
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
    for name in ["origin", "team/origin"] {
        let url = dir.path().join(format!("{}.git", name.replace('/', "-")));
        git(&root, &["remote", "add", name, url.to_str().unwrap()]);
        let tracking = format!("refs/remotes/{name}/feat");
        git(&root, &["update-ref", &tracking, &feat]);
    }
    git(&root, &["branch", "-q", "origin/feat"]);
    (dir, root)
}

#[test]
fn remote_shadowed_by_a_local_branch() {
    let (_dir, root) = init();
    let repo = open(&root);
    repo.switch_to(&remote("origin", "feat"), false).unwrap();
    assert_eq!(head(&root), "refs/heads/feat");
    assert_eq!(upstream(&root, "feat"), "origin refs/heads/feat");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "feat\n");

    repo.switch_to(&local("origin/feat"), false).unwrap();
    assert_eq!(head(&root), "refs/heads/origin/feat");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "main\n");

    // Its local branch, once there is one.
    repo.switch_to(&remote("origin", "feat"), false).unwrap();
    assert_eq!(head(&root), "refs/heads/feat");
}

#[test]
fn remote_name_with_a_slash() {
    let (_dir, root) = init();
    let repo = open(&root);
    repo.switch_to(&remote("team/origin", "feat"), false)
        .unwrap();
    assert_eq!(head(&root), "refs/heads/feat");
    assert_eq!(upstream(&root, "feat"), "team/origin refs/heads/feat");
}

#[test]
fn not_found() {
    let (_dir, root) = init();
    let repo = open(&root);
    for target in [
        local("nope"),
        remote("origin", "nope"),
        remote("nope", "feat"),
    ] {
        let error = repo.switch_to(&target, false).unwrap_err();
        assert!(
            matches!(error, SwitchError::NotFound),
            "{target:?}: {error}"
        );
        let error = repo.stash_and_switch_to(&target).unwrap_err();
        assert!(
            matches!(error, SwitchError::NotFound),
            "{target:?}: {error}"
        );
    }
    assert_eq!(head(&root), "refs/heads/main");
    assert_eq!(git(&root, &["stash", "list"]), "");
}

#[test]
fn overwrite_stash_and_force() {
    let (_dir, root) = init();
    let repo = open(&root);
    fs::write(root.join("a.txt"), "mine\n").unwrap();
    let error = repo
        .switch_to(&remote("origin", "feat"), false)
        .unwrap_err();
    let SwitchError::WouldOverwrite { paths } = error else {
        panic!("{error}");
    };
    assert_eq!(paths, [root.join("a.txt")]);
    assert_eq!(head(&root), "refs/heads/main");

    repo.stash_and_switch_to(&remote("origin", "feat")).unwrap();
    assert_eq!(head(&root), "refs/heads/feat");
    let stashes = git(&root, &["stash", "list"]);
    assert_eq!(stashes.lines().count(), 1, "{stashes}");
    assert!(
        stashes.contains("before switching to origin/feat"),
        "{stashes}"
    );

    git(&root, &["switch", "-q", "main"]);
    fs::write(root.join("a.txt"), "mine\n").unwrap();
    repo.switch_to(&local("feat"), true).unwrap();
    assert_eq!(head(&root), "refs/heads/feat");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "feat\n");
}
