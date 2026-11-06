//! A file's commit list across merges, against a temp repository built with the `git` CLI.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::Repo;

fn git_try(dir: &Path, n: u32, args: &[&str]) -> (bool, String) {
    let date = format!("{} +0000", 1_700_000_000 + n * 100);
    let output = Command::new("git")
        .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "rerere.enabled=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("run git");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned(),
    )
}

fn git(dir: &Path, n: u32, args: &[&str]) -> String {
    let (ok, out) = git_try(dir, n, args);
    assert!(ok, "git {args:?} failed");
    out
}

fn commit(root: &Path, n: u32, path: &str, text: &str) {
    fs::write(root.join(path), text).unwrap();
    git(root, n, &["add", "-A"]);
    git(root, n, &["commit", "-q", "-m", &format!("C{n}")]);
}

#[test]
fn a_merge_that_resolved_the_file_is_listed_as_git_log_lists_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, 0, &["init", "-q"]);
    commit(&root, 1, "f.txt", "a\nb\nc\n");
    git(&root, 2, &["checkout", "-q", "-b", "side"]);
    commit(&root, 3, "f.txt", "a\nB\nc\n");
    git(&root, 4, &["checkout", "-q", "main"]);
    commit(&root, 5, "f.txt", "a\nb2\nc\n");
    let (merged, _) = git_try(&root, 6, &["merge", "-q", "side"]);
    assert!(!merged, "the merge should conflict");
    fs::write(root.join("f.txt"), "a\nmerged\nc\n").unwrap();
    git(&root, 6, &["add", "f.txt"]);
    git(&root, 6, &["commit", "-q", "-m", "M"]);
    let m = git(&root, 6, &["rev-parse", "HEAD"]);
    git(&root, 7, &["checkout", "-q", "-b", "other"]);
    commit(&root, 8, "f.txt", "a\nmerged\nc\nd\n");
    git(&root, 9, &["checkout", "-q", "main"]);
    commit(&root, 10, "g.txt", "g\n");
    git(
        &root,
        11,
        &["merge", "-q", "--no-ff", "-m", "clean", "other"],
    );

    let expected: Vec<String> = git(&root, 12, &["log", "--format=%H", "--", "f.txt"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(expected.len(), 5);
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    let log = repo.file_log(&root.join("f.txt"), 100).unwrap();
    let ids: Vec<String> = log
        .iter()
        .map(|entry| entry.commit.id.to_string())
        .collect();
    assert_eq!(ids, expected);
    let entry = log.iter().find(|entry| *entry.commit.id == *m).unwrap();
    assert_eq!(entry.path, root.join("f.txt"));
    assert_eq!(entry.from, None);
}
