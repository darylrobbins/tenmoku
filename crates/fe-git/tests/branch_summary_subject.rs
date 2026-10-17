//! A branch's summary is its tip's subject as `git log --format=%s` gives it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use fe_git::Repo;

fn git(dir: &Path, args: &[&str], stdin: &[u8]) -> String {
    let mut child = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
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
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run git");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_owned()
}

#[test]
fn branch_summaries_are_the_subject_git_log_gives() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    git(path, &["init", "-q", "-b", "main"], b"");
    let tree = git(path, &["hash-object", "-t", "tree", "-w", "--stdin"], b"");
    let messages: [(&str, &[u8]); 3] = [
        ("wrapped", b"first line\nsecond line\n\nbody\n"),
        ("blank-lead", b"\n\n  lead  \nnext\n\nbody\n"),
        ("crlf", b"one\r\ntwo\r\n\r\nbody\r\n"),
    ];
    for (name, message) in messages {
        let commit = git(path, &["commit-tree", &tree], message);
        git(path, &["branch", name, &commit], b"");
    }
    let repo = Repo::discover(path).unwrap().expect("a repository");
    let branches = repo.branches().unwrap();
    for (name, _) in messages {
        let want = git(path, &["log", "-1", "--format=%s", name], b"");
        let branch = branches.iter().find(|b| b.name == name).expect("branch");
        assert_eq!(branch.summary, want, "{name}");
    }
}
