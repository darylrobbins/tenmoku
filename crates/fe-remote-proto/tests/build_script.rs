//! The build script's id without cargo: a committed tree's id, the
//! override, and no id without a commit (D § 2.5). tests/build_rerun.rs
//! covers how the id follows changes to the tree.

#[path = "../build_id.rs"]
#[allow(dead_code)]
mod build_id;

use build_id::{BuildId, Git, build_id};
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A repository with `crates/a/src/lib.rs` in two commits on `main`.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::create_dir_all(dir.path().join("crates/a/src")).unwrap();
    for text in ["one", "two"] {
        fs::write(dir.path().join("crates/a/src/lib.rs"), text).unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", text]);
    }
    dir
}

fn id_in(dir: &Path) -> BuildId {
    let git = Git {
        program: "git".into(),
        dir: dir.join("crates/a"),
    };
    build_id("1.2.3", None, &git).unwrap()
}

#[test]
fn a_clean_tree_is_its_commit() {
    let dir = repo();
    let id = id_in(dir.path()).id;
    let (version, commit) = id.split_once('+').unwrap();
    assert_eq!(version, "1.2.3");
    assert!(
        commit.len() == 12 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "{id}"
    );
}

#[test]
fn without_git_or_an_override_there_is_no_id() {
    let dir = tempfile::tempdir().unwrap();
    let git = Git {
        program: "git-that-is-not-installed".into(),
        dir: dir.path().to_owned(),
    };
    let err = build_id("1.2.3", None, &git).err().unwrap();
    assert!(err.contains("TENMOKU_BUILD_COMMIT"), "{err}");
    assert!(build_id("1.2.3", Some(" "), &git).is_err());
}

#[test]
fn the_override_names_the_commit_without_git() {
    let dir = tempfile::tempdir().unwrap();
    let git = Git {
        program: "git-that-is-not-installed".into(),
        dir: dir.path().to_owned(),
    };
    let build = build_id("1.2.3", Some("0123456789ab"), &git).unwrap();
    assert_eq!(build.id, "1.2.3+0123456789ab");
}
