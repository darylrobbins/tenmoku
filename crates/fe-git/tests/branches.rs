//! fe-git against repositories built with the `git` CLI in temp directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{Branch, Head, Repo};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = git_output(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(dir: &Path, args: &[&str]) -> std::process::Output {
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
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git")
}

/// A fresh repository with no commits.
fn init() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    (dir, root)
}

fn write(root: &Path, rela: &str, contents: &[u8]) {
    let path = root.join(rela);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn commit_at(root: &Path, file: &str, msg: &str, date: &str) {
    write(root, file, msg.as_bytes());
    git(root, &["add", "."]);
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(["commit", "-q", "-m", msg])
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_AUTHOR_DATE", date)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn lists_and_heads() {
    let (_dir, root) = init();
    git(&root, &["checkout", "-q", "-b", "main"]);
    assert_eq!(
        open(&root).current_branch().unwrap(),
        Head::Unborn("main".into())
    );
    assert_eq!(open(&root).head_message().unwrap(), None);

    commit_at(&root, "a", "first\n\nbody line\n", "2024-01-01T00:00:00Z");
    git(&root, &["checkout", "-q", "-b", "feat"]);
    commit_at(&root, "b", "newer", "2024-02-01T00:00:00Z");
    git(&root, &["checkout", "-q", "main"]);

    let clone_dir = tempfile::tempdir().unwrap();
    let clone = clone_dir.path().join("c");
    git(
        clone_dir.path(),
        &["clone", "-q", root.to_str().unwrap(), "c"],
    );
    git(&clone, &["checkout", "-q", "-b", "local-only"]);

    let repo = open(&root);
    assert_eq!(repo.current_branch().unwrap(), Head::Branch("main".into()));
    assert_eq!(
        repo.head_message().unwrap().as_deref(),
        Some("first\n\nbody line\n")
    );
    let names = |branches: &[Branch]| branches.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let listed = repo.branches().unwrap();
    assert_eq!(names(&listed), ["feat", "main"]);
    assert!(!listed[0].is_current && listed[1].is_current);
    assert_eq!(listed[0].summary, "newer");
    assert_eq!(listed[1].summary, "first");
    assert_eq!(listed[0].committed, 1706745600);

    let crepo = open(&clone);
    let listed = crepo.branches().unwrap();
    assert_eq!(
        names(&listed),
        ["local-only", "main", "origin/feat", "origin/main"]
    );
    assert_eq!(listed[1].upstream.as_deref(), Some("origin/main"));
    assert_eq!(listed[0].remote, None);
    assert_eq!(listed[3].remote.as_deref(), Some("origin"));
    assert!(listed.iter().all(|b| b.name != "origin/HEAD"));

    let id = repo.head_id().unwrap();
    git(&root, &["checkout", "-q", "--detach", &id]);
    assert_eq!(
        open(&root).current_branch().unwrap(),
        Head::Detached(id[..7].into())
    );
}
