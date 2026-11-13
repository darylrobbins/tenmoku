//! `head_text` and `index_text` drop a leading UTF-8 BOM, as the buffer does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::Repo;

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
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
}

/// A repository with `a.cs` committed as `bytes`.
fn repo_with(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, &["init", "-q"]);
    fs::write(root.join("a.cs"), bytes).unwrap();
    git(&root, &["add", "a.cs"]);
    git(&root, &["commit", "-q", "-m", "one"]);
    (dir, root)
}

#[test]
fn head_text_drops_a_utf8_bom() {
    let (_dir, root) = repo_with(b"\xEF\xBB\xBFx\ny\n");
    let repo = Repo::discover(&root).unwrap().expect("repo");
    assert_eq!(
        repo.head_text(&root.join("a.cs")).unwrap(),
        Some("x\ny\n".to_owned())
    );
}

#[test]
fn index_text_drops_a_utf8_bom() {
    let (_dir, root) = repo_with(b"\xEF\xBB\xBFx\ny\n");
    fs::write(root.join("a.cs"), b"\xEF\xBB\xBFx\nz\n").unwrap();
    git(&root, &["add", "a.cs"]);
    let repo = Repo::discover(&root).unwrap().expect("repo");
    assert_eq!(
        repo.index_text(&root.join("a.cs")).unwrap(),
        Some("x\nz\n".to_owned())
    );
}

#[test]
fn a_blob_without_a_bom_is_unchanged() {
    let (_dir, root) = repo_with(b"x\ny\n");
    let repo = Repo::discover(&root).unwrap().expect("repo");
    assert_eq!(
        repo.head_text(&root.join("a.cs")).unwrap(),
        Some("x\ny\n".to_owned())
    );
    assert_eq!(
        repo.index_text(&root.join("a.cs")).unwrap(),
        Some("x\ny\n".to_owned())
    );
    // A BOM-looking character that is not the BOM bytes is kept.
    fs::write(root.join("a.cs"), "\u{FFFE}x\n").unwrap();
    git(&root, &["add", "a.cs"]);
    assert_eq!(
        repo.index_text(&root.join("a.cs")).unwrap(),
        Some("\u{FFFE}x\n".to_owned())
    );
}
