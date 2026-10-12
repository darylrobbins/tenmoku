//! fe-git against repositories built with the `git` CLI in temp directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{ChangeKind, Repo, SplitStatus};

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

fn commit_all(root: &Path, msg: &str) {
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", msg]);
}

#[test]
fn index_text() {
    let (_dir, root) = init();
    write(&root, "a.txt", b"committed\n");
    write(&root, "bin.dat", b"\0\x01\x02");
    commit_all(&root, "init");
    write(&root, "a.txt", b"staged\n");
    git(&root, &["add", "a.txt"]);
    write(&root, "a.txt", b"on disk\n");
    write(&root, "loose.txt", b"untracked\n");
    let repo = open(&root);

    assert_eq!(
        repo.index_text(&root.join("a.txt")).unwrap().as_deref(),
        Some("staged\n")
    );
    assert_eq!(
        repo.head_text(&root.join("a.txt")).unwrap().as_deref(),
        Some("committed\n")
    );
    assert_eq!(repo.index_text(&root.join("loose.txt")).unwrap(), None);
    assert_eq!(repo.index_text(&root.join("bin.dat")).unwrap(), None);
    assert_eq!(
        repo.index_text(Path::new("/definitely/elsewhere.txt"))
            .unwrap(),
        None
    );
}

fn split_of(repo: &Repo) -> Vec<(String, Option<ChangeKind>, Option<ChangeKind>)> {
    let prefix = repo.workdir().to_path_buf();
    repo.status_split()
        .unwrap()
        .into_iter()
        .map(
            |SplitStatus {
                 path,
                 staged,
                 unstaged,
             }| {
                let rela = path
                    .strip_prefix(&prefix)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                (rela, staged, unstaged)
            },
        )
        .collect()
}

#[test]
fn split() {
    use ChangeKind::*;
    let (_dir, root) = init();
    for name in ["both", "staged", "unstaged", "gone"] {
        write(&root, name, b"one\n");
    }
    commit_all(&root, "init");
    write(&root, "both", b"two\n");
    git(&root, &["add", "both"]);
    write(&root, "both", b"three\n");
    write(&root, "staged", b"two\n");
    git(&root, &["add", "staged"]);
    write(&root, "unstaged", b"two\n");
    fs::remove_file(root.join("gone")).unwrap();
    write(&root, "new", b"x\n");
    git(&root, &["add", "new"]);
    write(&root, "untracked", b"y\n");

    assert_eq!(
        split_of(&open(&root)),
        vec![
            ("both".into(), Some(Modified), Some(Modified)),
            ("gone".into(), None, Some(Deleted)),
            ("new".into(), Some(Added), None),
            ("staged".into(), Some(Modified), None),
            ("unstaged".into(), None, Some(Modified)),
            ("untracked".into(), None, Some(Untracked)),
        ]
    );
}
