//! Committing a merge whose resolution matches HEAD, as `git commit` does.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::{CommitError, Repo};

fn git_ok(dir: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
    let (ok, out) = git_ok(dir, args);
    assert!(ok, "git {args:?} failed");
    out
}

#[test]
fn a_merge_resolved_to_head_commits_as_git_commit_does() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", ".git/hooks"),
    ] {
        git(root, &["config", k, v]);
    }
    fs::write(root.join("a.txt"), "init\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "side"]);
    fs::write(root.join("a.txt"), "side\n").unwrap();
    git(root, &["commit", "-qam", "side"]);
    git(root, &["checkout", "-q", "main"]);
    fs::write(root.join("a.txt"), "main\n").unwrap();
    git(root, &["commit", "-qam", "main"]);
    let old_head = git(root, &["rev-parse", "HEAD"]);
    let side = git(root, &["rev-parse", "side"]);
    let (merged, _) = git_ok(root, &["merge", "side"]);
    assert!(!merged, "the merge should conflict");
    git(root, &["checkout", "--ours", "a.txt"]);
    git(root, &["add", "a.txt"]);

    let repo = Repo::discover(root).unwrap().expect("a repository");
    assert!(!repo.has_staged().unwrap());
    let head = repo.commit("Merge branch 'side'", false).unwrap();

    assert_eq!(head, git(root, &["rev-parse", "HEAD"]));
    assert_eq!(
        git(root, &["rev-list", "--parents", "-n1", "HEAD"]),
        format!("{head} {old_head} {side}")
    );
    assert!(!root.join(".git/MERGE_HEAD").exists());

    let again = repo.commit("Nothing more", false);
    assert!(
        matches!(again, Err(CommitError::NothingStaged)),
        "{again:?}"
    );
}
