//! A commit's summary skips blank lines before the first line with text, as `git blame` does.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::Repo;

fn git(dir: &Path, args: &[&str]) -> String {
    let date = "1700000000 +0100";
    let output = Command::new("git")
        .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

fn commit_verbatim(root: &Path, text: &str, message: &str) {
    fs::write(root.join("a.txt"), text).unwrap();
    git(root, &["add", "-A"]);
    fs::write(root.join("msg.tmp"), message).unwrap();
    git(
        root,
        &["commit", "-q", "--cleanup=verbatim", "-F", "msg.tmp"],
    );
    fs::remove_file(root.join("msg.tmp")).unwrap();
}

#[test]
fn summary_skips_leading_blank_lines_as_git_blame_does() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, &["init", "-q"]);
    let raw = "\n  \n\tSubject here  \nmore\n\nBody\n";
    commit_verbatim(&root, "one\n", raw);
    commit_verbatim(&root, "one\ntwo\n", "Plain\n");
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    let path = root.join("a.txt");

    let runs = repo.blame(&path, "one\ntwo\n").unwrap();
    let summaries: Vec<&str> = runs
        .iter()
        .map(|run| run.commit.as_ref().unwrap().summary.as_str())
        .collect();
    assert_eq!(summaries, ["\tSubject here", "Plain"]);
    assert_eq!(runs[0].commit.as_ref().unwrap().message, raw);

    let log = repo.file_log(&path, 10).unwrap();
    let summaries: Vec<&str> = log.iter().map(|e| e.commit.summary.as_str()).collect();
    assert_eq!(summaries, ["Plain", "\tSubject here"]);
}
