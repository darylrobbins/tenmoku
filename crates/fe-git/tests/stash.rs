//! Stashing, listing, popping and applying, against temp repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{Repo, StashEntry, StashError};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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

/// A repo on `main` with `a.txt` committed, then changed, and `new.txt`
/// untracked. Identity is set locally so `git stash` works from the
/// library.
fn init() -> (tempfile::TempDir, PathBuf, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    fs::write(root.join("a.txt"), "one\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.txt"), "two\n").unwrap();
    fs::write(root.join("new.txt"), "new\n").unwrap();
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    (dir, root, repo)
}

/// Each entry's index, message and branch.
fn listed(repo: &Repo) -> Vec<(usize, String, Option<String>)> {
    (repo.stash_list().unwrap().into_iter())
        .map(
            |StashEntry {
                 index,
                 message,
                 branch,
                 ..
             }| (index, message, branch),
        )
        .collect()
}

fn read(root: &Path, name: &str) -> String {
    fs::read_to_string(root.join(name)).unwrap()
}

#[test]
fn push_list_pop() {
    let (_dir, root, repo) = init();
    assert_eq!(repo.stash_list().unwrap(), []);

    repo.stash_push(Some("wip"), false).unwrap();
    assert_eq!(read(&root, "a.txt"), "one\n");
    assert_eq!(read(&root, "new.txt"), "new\n");
    let main = Some("main".to_owned());
    assert_eq!(listed(&repo), [(0, "wip".to_owned(), main)]);

    repo.stash_pop(0).unwrap();
    assert_eq!(read(&root, "a.txt"), "two\n");
    assert_eq!(repo.stash_list().unwrap(), []);
}

#[test]
fn include_untracked_and_apply() {
    let (_dir, root, repo) = init();
    repo.stash_push(None, true).unwrap();
    assert_eq!(read(&root, "a.txt"), "one\n");
    assert!(!root.join("new.txt").exists());
    // Given no message, git's.
    let head = git(&root, &["rev-parse", "--short", "HEAD"]);
    let entry = (0, format!("{head} init"), Some("main".to_owned()));
    assert_eq!(listed(&repo), std::slice::from_ref(&entry));

    repo.stash_apply(0).unwrap();
    assert_eq!(read(&root, "a.txt"), "two\n");
    assert_eq!(read(&root, "new.txt"), "new\n");
    assert_eq!(listed(&repo), std::slice::from_ref(&entry));

    // A pop that conflicts names the file and keeps the entry.
    fs::remove_file(root.join("new.txt")).unwrap();
    fs::write(root.join("a.txt"), "three\n").unwrap();
    git(&root, &["commit", "-q", "-am", "three"]);
    let Err(StashError::Conflict { conflicts }) = repo.stash_pop(0) else {
        panic!("a conflict");
    };
    assert_eq!(conflicts, ["CONFLICT (content): Merge conflict in a.txt"]);
    let error = StashError::Conflict { conflicts }.to_string();
    assert!(error.contains("Merge conflict in a.txt"), "{error}");
    assert!(read(&root, "a.txt").contains("<<<<<<<"));
    assert_eq!(listed(&repo), [entry]);
}

#[test]
fn list_latest_first() {
    let (_dir, root, repo) = init();
    repo.stash_push(Some("first"), false).unwrap();
    fs::write(root.join("a.txt"), "three\n").unwrap();
    repo.stash_push(Some("second"), false).unwrap();
    let main = Some("main".to_owned());
    assert_eq!(
        listed(&repo),
        [
            (0, "second".to_owned(), main.clone()),
            (1, "first".to_owned(), main.clone())
        ]
    );

    // By index: the older one.
    repo.stash_pop(1).unwrap();
    assert_eq!(read(&root, "a.txt"), "two\n");
    assert_eq!(listed(&repo), [(0, "second".to_owned(), main)]);
    let ids = |repo: &Repo| -> Vec<String> {
        (repo.stash_list().unwrap().into_iter())
            .map(|entry| entry.id)
            .collect()
    };
    let second = ids(&repo)[0].clone();
    assert_eq!(second, git(&root, &["rev-parse", "stash@{0}"]));

    // Its id stays as entries come above it.
    repo.stash_push(None, true).unwrap();
    assert_eq!(ids(&repo)[1], second);
}

#[test]
fn nothing_to_stash() {
    let (_dir, root, repo) = init();
    fs::remove_file(root.join("new.txt")).unwrap();
    git(&root, &["checkout", "--", "a.txt"]);
    let error = repo.stash_push(Some("wip"), false).unwrap_err();
    assert!(matches!(error, StashError::NothingToStash), "{error:?}");
    assert_eq!(error.to_string(), "No local changes to save");
    assert_eq!(repo.stash_list().unwrap(), []);

    // An untracked file alone isn't a change unless it's included.
    fs::write(root.join("new.txt"), "new\n").unwrap();
    let error = repo.stash_push(None, false).unwrap_err();
    assert!(matches!(error, StashError::NothingToStash), "{error:?}");
    assert_eq!(repo.stash_list().unwrap(), []);

    // Popping an entry that isn't there changes nothing.
    let error = repo.stash_pop(0).unwrap_err();
    assert!(matches!(error, StashError::Failed { .. }), "{error:?}");
}

/// Stash & Checkout's stash takes the untracked files out of the tree too,
/// so popping it on the way back restores them.
#[test]
fn stash_and_switch_then_pop() {
    let (_dir, root, repo) = init();
    git(&root, &["branch", "-q", "feat"]);
    repo.stash_and_switch("feat").unwrap();
    assert_eq!(read(&root, "a.txt"), "one\n");
    assert!(!root.join("new.txt").exists());
    let messages: Vec<String> = (repo.stash_list().unwrap().into_iter())
        .map(|entry| entry.message)
        .collect();
    assert_eq!(messages, ["tenmoku: before switching to feat"]);

    repo.stash_pop(0).unwrap();
    assert_eq!(read(&root, "a.txt"), "two\n");
    assert_eq!(read(&root, "new.txt"), "new\n");
    assert_eq!(repo.stash_list().unwrap(), []);
}

#[test]
fn a_message_that_says_nothing_to_save_is_still_stashed() {
    let (_dir, _root, repo) = init();
    repo.stash_push(Some("No local changes to save"), false)
        .unwrap();
    assert_eq!(
        listed(&repo),
        [(
            0,
            "No local changes to save".to_owned(),
            Some("main".to_owned())
        )]
    );

    // With no message, git's default holds the HEAD subject.
    let (_dir, root, repo) = init();
    git(&root, &["stash", "-q", "-u"]);
    git(
        &root,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "No local changes to save",
        ],
    );
    git(&root, &["stash", "pop", "-q"]);
    repo.stash_push(None, true).unwrap();
    let entries = repo.stash_list().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].message.contains("No local changes to save"));
}
