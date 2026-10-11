//! fe-git against repositories built with the `git` CLI in temp directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{ChangeKind, FileStatus, Repo};

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

fn status(repo: &Repo) -> Vec<(PathBuf, ChangeKind)> {
    repo.status()
        .unwrap()
        .into_iter()
        .map(|FileStatus { path, kind }| (path, kind))
        .collect()
}

#[test]
fn status_lists_each_kind_of_change_and_skips_ignored_and_touched_files() {
    let (_dir, root) = init();
    write(&root, ".gitignore", b"*.log\n");
    write(&root, "modified.txt", b"one\n");
    write(&root, "deleted.txt", b"gone soon\n");
    write(&root, "touched.txt", b"same\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);

    write(&root, "modified.txt", b"two\n");
    fs::remove_file(root.join("deleted.txt")).unwrap();
    write(&root, "added.txt", b"new\n");
    git(&root, &["add", "added.txt"]);
    write(&root, "newdir/untracked.txt", b"loose\n");
    write(&root, "debug.log", b"ignored\n");
    // Same content, new mtime: the stat changes but the file doesn't.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write(&root, "touched.txt", b"same\n");

    let repo = open(&root);
    let workdir = repo.workdir().to_path_buf();
    assert_eq!(
        status(&repo),
        vec![
            (workdir.join("added.txt"), ChangeKind::Added),
            (workdir.join("deleted.txt"), ChangeKind::Deleted),
            (workdir.join("modified.txt"), ChangeKind::Modified),
            (workdir.join("newdir/untracked.txt"), ChangeKind::Untracked),
        ]
    );
    assert!(status(&repo).iter().all(|(path, _)| path.is_absolute()));
}

#[test]
fn unmerged_file_is_conflicted_and_status_leaves_the_index_alone() {
    let (_dir, root) = init();
    write(&root, "a.txt", b"base\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "other"]);
    write(&root, "a.txt", b"other\n");
    git(&root, &["commit", "-q", "-am", "other"]);
    git(&root, &["checkout", "-q", "main"]);
    write(&root, "a.txt", b"main\n");
    git(&root, &["commit", "-q", "-am", "main"]);
    let merge = git_output(&root, &["merge", "-q", "other"]);
    assert!(!merge.status.success(), "the merge should conflict");

    let index = root.join(".git/index");
    let before = fs::read(&index).unwrap();
    let repo = open(&root);
    assert_eq!(
        status(&repo),
        vec![(repo.workdir().join("a.txt"), ChangeKind::Conflicted)]
    );
    assert_eq!(fs::read(&index).unwrap(), before);
}

#[test]
fn head_text_and_head_id() {
    let (_dir, root) = init();
    write(&root, "text.txt", b"committed\n");
    write(&root, "binary.bin", b"a\0b");
    write(&root, "untracked.txt", b"loose\n");

    // No commits yet: nothing at HEAD.
    let repo = open(&root);
    let workdir = repo.workdir().to_path_buf();
    assert_eq!(repo.head_id(), None);
    assert_eq!(repo.head_text(&workdir.join("text.txt")).unwrap(), None);

    git(&root, &["add", "text.txt", "binary.bin"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let first = repo.head_id().expect("HEAD after a commit");
    write(&root, "text.txt", b"edited\n");

    assert_eq!(
        repo.head_text(&workdir.join("text.txt"))
            .unwrap()
            .as_deref(),
        Some("committed\n")
    );
    assert_eq!(
        repo.head_text(&workdir.join("untracked.txt")).unwrap(),
        None
    );
    assert_eq!(repo.head_text(&workdir.join("binary.bin")).unwrap(), None);
    assert_eq!(
        repo.head_text(Path::new("/elsewhere/text.txt")).unwrap(),
        None
    );

    git(&root, &["commit", "-q", "-am", "second"]);
    let second = repo.head_id().expect("HEAD after a second commit");
    assert_ne!(first, second);
    assert_eq!(
        repo.head_text(&workdir.join("text.txt"))
            .unwrap()
            .as_deref(),
        Some("edited\n")
    );
}

#[test]
fn discover_finds_the_repo_from_a_subdirectory_and_nothing_outside_one() {
    let outside = tempfile::tempdir().unwrap();
    // A temp dir could sit inside a repository of its own; only check when it doesn't.
    if Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(outside.path())
        .output()
        .is_ok_and(|output| !output.status.success())
    {
        assert!(Repo::discover(outside.path()).unwrap().is_none());
    }

    let (_dir, root) = init();
    let sub = root.join("a/b");
    fs::create_dir_all(&sub).unwrap();
    let repo = Repo::discover(&sub)
        .unwrap()
        .expect("found from a subdirectory");
    assert_eq!(repo.workdir(), root);
}
