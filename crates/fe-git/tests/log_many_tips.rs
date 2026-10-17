//! The All Branches log in a repository with more refs than fit in git's
//! argument list.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use fe_git::{LogScope, Repo};

const COMMITS: usize = 60_000;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
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
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

#[test]
fn all_branches_log_reads_more_tips_than_fit_in_arguments() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);

    let mut stream = String::new();
    for i in 1..=COMMITS {
        write!(
            stream,
            "commit refs/heads/main\nmark :{i}\ncommitter t <t@t> {i} +0000\ndata 1\nc\n"
        )
        .unwrap();
        if i > 1 {
            writeln!(stream, "from :{}", i - 1).unwrap();
        }
        stream.push('\n');
    }
    let marks = root.join("marks");
    let mut child = Command::new("git")
        .args(["fast-import", "--quiet"])
        .arg(format!("--export-marks={}", marks.display()))
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn fast-import");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .expect("feed fast-import");
    assert!(child.wait().expect("fast-import").success());

    // One tag per commit, written as a packed-refs file sorted by name.
    let mut tags: Vec<(String, String)> = std::fs::read_to_string(&marks)
        .expect("marks")
        .lines()
        .map(|line| {
            let (mark, id) = line.split_once(' ').expect("mark line");
            (
                format!(
                    "refs/tags/t{:06}",
                    mark.trim_start_matches(':').parse::<usize>().unwrap()
                ),
                id.to_owned(),
            )
        })
        .collect();
    tags.sort();
    let mut packed = String::from("# pack-refs with: peeled fully-peeled sorted \n");
    for (name, id) in &tags {
        writeln!(packed, "{id} {name}").unwrap();
    }
    std::fs::write(root.join(".git/packed-refs"), packed).expect("packed-refs");
    std::fs::remove_file(&marks).expect("remove marks");
    git(root, &["reset", "-q", "--hard", "main"]);

    let repo = Repo::discover(root).unwrap().unwrap();
    let tips = repo.log_tips(LogScope::All).expect("tips");
    assert_eq!(tips.len(), COMMITS);

    let commits = repo.log_from(&tips, 1, 3).expect("log_from");
    let ids: Vec<String> = commits.iter().map(|c| c.commit.id.clone()).collect();
    let expected = git(
        root,
        &[
            "log",
            "--all",
            "--topo-order",
            "--skip=1",
            "--max-count=3",
            "--format=%H",
        ],
    );
    assert_eq!(ids.join("\n"), expected);
}
