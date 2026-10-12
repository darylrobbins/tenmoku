//! Switching and creating branches against temp repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{Repo, SwitchError};

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

fn write(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
}

fn commit(root: &Path, rela: &str, text: &str) {
    write(root, rela, text);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn head(root: &Path) -> String {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// A repo on `main` with files a and b, and a branch `other` that changes b.
/// Identity is set locally so `git stash` works from the library.
fn init() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t"]);
    commit(&root, "a", "a1");
    commit(&root, "b", "b1");
    git(&root, &["branch", "other"]);
    git(&root, &["checkout", "-q", "other"]);
    commit(&root, "b", "b2");
    git(&root, &["checkout", "-q", "main"]);
    (dir, root)
}

#[test]
fn switches() {
    let (_dir, root) = init();
    let repo = open(&root);

    write(&root, "a", "local edit");
    repo.switch("other", false).unwrap();
    assert_eq!(head(&root), "other");
    assert_eq!(fs::read_to_string(root.join("a")).unwrap(), "local edit");
    git(&root, &["checkout", "-q", "--", "a"]);
    repo.switch("main", false).unwrap();

    // A remote branch: `feat` exists only on origin.
    let origin = tempfile::tempdir().unwrap();
    git(&root, &["checkout", "-q", "-b", "feat"]);
    commit(&root, "f", "f1");
    git(&root, &["checkout", "-q", "main"]);
    git(
        origin.path(),
        &["clone", "-q", "--bare", root.to_str().unwrap(), "o.git"],
    );
    git(&root, &["branch", "-q", "-D", "feat"]);
    let url = origin.path().join("o.git");
    git(&root, &["remote", "add", "origin", url.to_str().unwrap()]);
    git(&root, &["fetch", "-q", "origin"]);

    repo.switch("origin/feat", false).unwrap();
    assert_eq!(head(&root), "feat");
    assert_eq!(
        git(&root, &["rev-parse", "--abbrev-ref", "feat@{upstream}"]),
        "origin/feat"
    );
    repo.switch("main", false).unwrap();
    repo.switch("origin/feat", false).unwrap();
    assert_eq!(head(&root), "feat");

    repo.create_branch("fresh").unwrap();
    assert_eq!(head(&root), "fresh");
    assert_eq!(
        git(&root, &["rev-parse", "fresh"]),
        git(&root, &["rev-parse", "feat"])
    );
}

#[test]
fn would_overwrite() {
    let (_dir, root) = init();
    let repo = open(&root);
    let main_id = git(&root, &["rev-parse", "HEAD"]);

    write(&root, "b", "local edit");
    let err = repo.switch("other", false).unwrap_err();
    match err {
        SwitchError::WouldOverwrite { paths } => assert_eq!(paths, vec![repo.workdir().join("b")]),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(head(&root), "main");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), main_id);
    assert_eq!(fs::read_to_string(root.join("b")).unwrap(), "local edit");

    repo.stash_and_switch("other").unwrap();
    assert_eq!(head(&root), "other");
    let stashes = git(&root, &["stash", "list"]);
    assert_eq!(stashes.lines().count(), 1);
    assert!(
        stashes.contains("fastedit: before switching to other"),
        "{stashes}"
    );

    git(&root, &["checkout", "-q", "main"]);
    git(&root, &["stash", "drop", "-q"]);
    write(&root, "b", "local edit");
    repo.switch("other", true).unwrap();
    assert_eq!(head(&root), "other");
    assert_eq!(fs::read_to_string(root.join("b")).unwrap(), "b2");
}

#[test]
fn errors() {
    let (_dir, root) = init();
    let repo = open(&root);
    let branches = git(&root, &["branch", "--list"]);

    assert!(matches!(
        repo.create_branch("bad..name"),
        Err(SwitchError::InvalidName)
    ));
    assert!(matches!(
        repo.switch("nope", false),
        Err(SwitchError::NotFound)
    ));
    assert_eq!(head(&root), "main");
    assert_eq!(git(&root, &["branch", "--list"]), branches);
}
