//! Creating a branch at a ref, renaming and deleting branches, in real
//! repositories: a clone `work` of `origin`, whose `main` has file `f` and
//! whose `feat` adds file `g`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{BranchError, BranchRef, Repo, SwitchError};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = run_git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
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

fn commit(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

/// The value of config `key` in `root`, `None` when unset.
fn config(root: &Path, key: &str) -> Option<String> {
    let output = run_git(root, &["config", "--get", key]);
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn has_ref(root: &Path, name: &str) -> bool {
    run_git(root, &["show-ref", "--verify", "--quiet", name])
        .status
        .success()
}

fn head(root: &Path) -> String {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

/// `work`, a clone of `origin`, on `main` tracking `origin/main`.
fn clone() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q"]);
    commit(&origin, "f", "f\n");
    git(&origin, &["checkout", "-q", "-b", "feat"]);
    commit(&origin, "g", "g\n");
    git(&origin, &["checkout", "-q", "main"]);
    git(dir.path(), &["clone", "-q", "origin", "work"]);
    let work = dir.path().join("work");
    (dir, work)
}

fn remote(remote: &str, branch: &str) -> BranchRef {
    BranchRef::Remote {
        remote: remote.into(),
        branch: branch.into(),
    }
}

#[test]
fn create_branch_at_a_remote_ref_tracks_it() {
    let (_dir, work) = clone();
    let repo = open(&work);

    repo.create_branch_at("mine", &remote("origin", "feat"), false)
        .unwrap();
    assert_eq!(head(&work), "main", "made without switching");
    assert_eq!(
        git(&work, &["rev-parse", "mine"]),
        git(&work, &["rev-parse", "origin/feat"])
    );
    assert_eq!(
        config(&work, "branch.mine.remote").as_deref(),
        Some("origin")
    );
    assert_eq!(
        config(&work, "branch.mine.merge").as_deref(),
        Some("refs/heads/feat")
    );

    // Tracking doesn't depend on branch.autoSetupMerge.
    git(&work, &["config", "branch.autoSetupMerge", "false"]);
    repo.create_branch_at("theirs", &remote("origin", "feat"), true)
        .unwrap();
    assert_eq!(head(&work), "theirs", "switched to");
    assert!(work.join("g").exists());
    assert_eq!(
        config(&work, "branch.theirs.merge").as_deref(),
        Some("refs/heads/feat")
    );

    // A local start isn't tracked, even when the config says always.
    git(&work, &["config", "branch.autoSetupMerge", "always"]);
    repo.create_branch_at("local", &BranchRef::Local("main".into()), false)
        .unwrap();
    assert_eq!(
        git(&work, &["rev-parse", "local"]),
        git(&work, &["rev-parse", "main"])
    );
    assert_eq!(config(&work, "branch.local.remote"), None);
    assert_eq!(head(&work), "theirs");
}

#[test]
fn create_branch_at_refuses_without_changing_anything() {
    let (_dir, work) = clone();
    let repo = open(&work);
    let main = BranchRef::Local("main".into());

    assert!(matches!(
        repo.create_branch_at("main", &remote("origin", "feat"), true),
        Err(SwitchError::Exists)
    ));
    for name in ["@{-1}", "a..b", "-x", ""] {
        assert!(
            matches!(
                repo.create_branch_at(name, &main, false),
                Err(SwitchError::InvalidName)
            ),
            "{name:?}"
        );
    }
    assert!(matches!(
        repo.create_branch_at("new", &remote("origin", "nope"), true),
        Err(SwitchError::NotFound)
    ));
    assert!(matches!(
        repo.create_branch_at("new", &BranchRef::Local("feat".into()), true),
        Err(SwitchError::NotFound)
    ));
    assert!(!has_ref(&work, "refs/heads/new"));

    // Local changes the start would overwrite stop the switch, and leave no
    // branch behind.
    fs::write(work.join("g"), "untracked").unwrap();
    match repo.create_branch_at("new", &remote("origin", "feat"), true) {
        Err(SwitchError::WouldOverwrite { paths }) => {
            assert_eq!(paths, vec![repo.workdir().join("g")]);
        }
        other => panic!("expected WouldOverwrite, got {other:?}"),
    }
    assert_eq!(head(&work), "main");
    assert!(!has_ref(&work, "refs/heads/new"));
    assert_eq!(fs::read_to_string(work.join("g")).unwrap(), "untracked");
}

#[test]
fn rename_moves_upstream_config() {
    let (_dir, work) = clone();
    let repo = open(&work);

    repo.rename_branch("main", "trunk").unwrap();
    assert_eq!(head(&work), "trunk", "HEAD moves with its branch");
    assert!(!has_ref(&work, "refs/heads/main"));
    assert_eq!(
        config(&work, "branch.trunk.remote").as_deref(),
        Some("origin")
    );
    assert_eq!(
        config(&work, "branch.trunk.merge").as_deref(),
        Some("refs/heads/main")
    );
    assert_eq!(config(&work, "branch.main.remote"), None);
    assert_eq!(config(&work, "branch.main.merge"), None);

    // A branch HEAD isn't on.
    git(&work, &["branch", "-q", "side"]);
    repo.rename_branch("side", "other").unwrap();
    assert!(has_ref(&work, "refs/heads/other"));
    assert!(!has_ref(&work, "refs/heads/side"));
    assert_eq!(head(&work), "trunk");
}

#[test]
fn rename_refuses_without_changing_anything() {
    let (_dir, work) = clone();
    let repo = open(&work);
    git(&work, &["branch", "-q", "side"]);

    assert!(matches!(
        repo.rename_branch("side", "main"),
        Err(BranchError::Exists)
    ));
    for name in ["@{-1}", "a..b", "-x", "a b", ""] {
        assert!(
            matches!(
                repo.rename_branch("side", name),
                Err(BranchError::InvalidName)
            ),
            "{name:?}"
        );
    }
    assert!(matches!(
        repo.rename_branch("nope", "new"),
        Err(BranchError::NotFound)
    ));
    // Remote-tracking branches aren't renamed.
    assert!(matches!(
        repo.rename_branch("origin/feat", "new"),
        Err(BranchError::NotFound)
    ));
    assert!(has_ref(&work, "refs/heads/side"));
    assert!(has_ref(&work, "refs/heads/main"));
    assert!(!has_ref(&work, "refs/heads/new"));
    // To its own name, nothing to do.
    repo.rename_branch("side", "side").unwrap();
    assert!(has_ref(&work, "refs/heads/side"));
}

#[test]
fn delete_current_is_refused() {
    let (_dir, work) = clone();
    let repo = open(&work);

    for force in [false, true] {
        assert!(matches!(
            repo.delete_branch("main", force),
            Err(BranchError::Current)
        ));
    }
    assert!(has_ref(&work, "refs/heads/main"));
    assert_eq!(
        config(&work, "branch.main.remote").as_deref(),
        Some("origin")
    );

    assert!(matches!(
        repo.delete_branch("nope", true),
        Err(BranchError::NotFound)
    ));
    assert!(matches!(
        repo.delete_branch("origin/feat", true),
        Err(BranchError::NotFound)
    ));
    assert!(has_ref(&work, "refs/remotes/origin/feat"));

    // Detached, every branch can go.
    git(&work, &["checkout", "-q", "--detach"]);
    repo.delete_branch("main", false).unwrap();
    assert!(!has_ref(&work, "refs/heads/main"));
}

#[test]
fn delete_unmerged_needs_force() {
    let (_dir, work) = clone();
    let repo = open(&work);
    git(&work, &["checkout", "-q", "-b", "topic"]);
    commit(&work, "t", "t\n");
    git(&work, &["config", "branch.topic.description", "work"]);
    git(&work, &["checkout", "-q", "main"]);
    git(&work, &["branch", "-q", "merged"]);

    assert!(matches!(
        repo.delete_branch("topic", false),
        Err(BranchError::NotMerged)
    ));
    assert!(has_ref(&work, "refs/heads/topic"));

    repo.delete_branch("topic", true).unwrap();
    assert!(!has_ref(&work, "refs/heads/topic"));
    assert_eq!(config(&work, "branch.topic.description"), None);

    // Merged, no force is needed.
    repo.delete_branch("merged", false).unwrap();
    assert!(!has_ref(&work, "refs/heads/merged"));
}

#[test]
fn delete_a_branch_checked_out_elsewhere_fails() {
    let (dir, work) = clone();
    let repo = open(&work);
    let other = dir.path().join("other");
    git(
        &work,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "elsewhere",
            other.to_str().unwrap(),
        ],
    );
    match repo.delete_branch("elsewhere", true) {
        // Git words this by version: "used by worktree at" or "checked out at".
        Err(BranchError::Failed { output }) => assert!(
            output.contains("worktree") || output.contains("checked out"),
            "{output}"
        ),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(has_ref(&work, "refs/heads/elsewhere"));
}
