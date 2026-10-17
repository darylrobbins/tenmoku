//! Staging, unstaging and discarding a file whose name is decomposed (NFD) on disk.
#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::Repo;

const NFD: &str = "cafe\u{301}.txt";

/// Runs `git` in `dir`, with the developer's own config shut out.
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
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository with `NFD` committed as "one\n" and then edited to "two\n".
fn fixture(precompose: &str) -> (tempfile::TempDir, PathBuf, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "core.precomposeUnicode", precompose]);
    fs::write(root.join(NFD), "one\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join(NFD), "two\n").unwrap();
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    (dir, root, repo)
}

fn status(root: &Path) -> String {
    git(root, &["-c", "core.quotepath=off", "status", "--porcelain"])
}

#[test]
fn discarding_a_decomposed_name_restores_the_tracked_file() {
    let (_dir, root, repo) = fixture("true");
    repo.discard(&[root.join(NFD)]).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(NFD)).ok().as_deref(),
        Some("one\n")
    );
}

#[test]
fn staging_a_decomposed_name_stages_it() {
    let (_dir, root, repo) = fixture("true");
    repo.stage(&[root.join(NFD)]).unwrap();
    assert_eq!(status(&root), "M  caf\u{e9}.txt\n");
}

#[test]
fn unstaging_a_decomposed_name_unstages_it() {
    let (_dir, root, repo) = fixture("true");
    git(&root, &["add", "-A"]);
    repo.unstage(&[root.join(NFD)]).unwrap();
    assert_eq!(status(&root), " M caf\u{e9}.txt\n");
}

#[test]
fn a_decomposed_index_name_without_precompose_still_stages() {
    let (_dir, root, repo) = fixture("false");
    repo.stage(&[root.join(NFD)]).unwrap();
    assert_eq!(status(&root), "M  cafe\u{301}.txt\n");
}
