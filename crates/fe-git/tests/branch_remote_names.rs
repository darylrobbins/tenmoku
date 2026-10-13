//! A remote whose name contains a slash names its branches' remote whole.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::Repo;

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
            "-c",
            "protocol.file.allow=always",
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

/// `upstream.git` holds one commit on main; `local` is a clone with a second
/// remote `team/origin` fetched and its HEAD set.
fn setup() -> (tempfile::TempDir, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    let upstream = base.join("upstream.git");
    let seed = base.join("seed");
    let local = base.join("local");
    fs::create_dir(&upstream).unwrap();
    git(&upstream, &["init", "-q", "--bare", "-b", "main"]);
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    fs::write(seed.join("a"), "a").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "a"]);
    git(&seed, &["push", "-q", upstream.to_str().unwrap(), "main"]);
    git(base, &["clone", "-q", upstream.to_str().unwrap(), "local"]);
    git(&local, &["remote", "add", "team/origin", "../upstream.git"]);
    git(&local, &["fetch", "-q", "team/origin"]);
    git(&local, &["remote", "set-head", "team/origin", "main"]);
    let repo = Repo::discover(&local).unwrap().expect("a repository");
    (dir, repo)
}

fn remote_of(repo: &Repo, name: &str) -> Option<String> {
    let branches = repo.branches().unwrap();
    let branch = branches
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("{name} not listed"));
    branch.remote.clone()
}

#[test]
fn remote_with_a_slash_names_its_branches_remote() {
    let (_dir, repo) = setup();
    assert_eq!(
        remote_of(&repo, "team/origin/main").as_deref(),
        Some("team/origin")
    );
    assert_eq!(remote_of(&repo, "origin/main").as_deref(), Some("origin"));
}

#[test]
fn remote_head_of_a_remote_with_a_slash_is_not_listed() {
    let (_dir, repo) = setup();
    for branch in repo.branches().unwrap() {
        assert!(!branch.name.ends_with("/HEAD"), "{} listed", branch.name);
    }
}

#[test]
fn remote_branch_with_a_slash_keeps_its_remote() {
    let (dir, repo) = setup();
    let local = dir.path().join("local");
    git(&local, &["branch", "feature/x"]);
    git(&local, &["push", "-q", "origin", "feature/x"]);
    assert_eq!(
        remote_of(&repo, "origin/feature/x").as_deref(),
        Some("origin")
    );
}
