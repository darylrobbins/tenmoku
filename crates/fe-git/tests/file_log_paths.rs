//! File history and blame for non-UTF-8 names and with `log.showRoot=false`,
//! against temp repositories built with the `git` CLI.

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

/// Writes each `(name, text)`, commits everything and returns the id.
fn commit(root: &Path, files: &[(&Path, &str)], summary: &str) -> String {
    for (name, text) in files {
        fs::write(root.join(name), text).unwrap();
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", summary]);
    git(root, &["rev-parse", "HEAD"])
}

const BODY: &str = "alpha\nbeta\ngamma\n";

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_file_names() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, &["init", "-q"]);
    let name = Path::new(OsStr::from_bytes(b"caf\xe9.rs"));
    let c1 = commit(&root, &[(name, BODY)], "C1");
    let text = format!("{BODY}delta\n");
    let c2 = commit(&root, &[(name, &text)], "C2");
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    let path = root.join(name);

    let log = repo.file_log(&path, 10).unwrap();
    let ids: Vec<_> = log
        .iter()
        .map(|entry| entry.commit.id.to_string())
        .collect();
    assert_eq!(ids, [c2, c1.clone()]);
    for entry in &log {
        assert_eq!(
            entry.path.as_os_str().as_bytes(),
            path.as_os_str().as_bytes()
        );
    }

    let runs = repo.blame(&path, &text).unwrap();
    let blamed: Vec<_> = runs
        .iter()
        .map(|run| run.commit.as_ref().map(|c| c.id.to_string()))
        .collect();
    assert_eq!(blamed.len(), 2, "{runs:?}");
    assert_eq!(blamed[0].as_deref(), Some(c1.as_str()));
    assert_eq!(blamed[1].as_deref(), Some(ids[0].as_str()));
}

#[test]
fn root_commit_listed_with_show_root_off() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "log.showRoot", "false"]);
    let (a, only_root) = (Path::new("a.rs"), Path::new("only_root.rs"));
    let c1 = commit(&root, &[(a, BODY), (only_root, "root\n")], "C1");
    let c2 = commit(&root, &[(a, &format!("{BODY}delta\n"))], "C2");
    let repo = Repo::discover(&root).unwrap().expect("a repository");

    let ids = |name: &Path| -> Vec<String> {
        repo.file_log(&root.join(name), 10)
            .unwrap()
            .iter()
            .map(|entry| entry.commit.id.to_string())
            .collect()
    };
    assert_eq!(ids(a), [c2, c1.clone()]);
    assert_eq!(ids(only_root), [c1]);
}
