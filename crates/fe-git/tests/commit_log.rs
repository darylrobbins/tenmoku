//! A repository's commit log and the files each commit changed, against temp
//! repositories built with the `git` CLI.

mod commit_log {
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use fe_git::{
        ChangeKind, CommitFile, CommitInfo, LogCommit, LogScope, RefKind, RefLabel, Repo,
    };

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

    /// Writes each `(path, text)`, commits everything and returns the id.
    fn commit(root: &Path, files: &[(&str, &str)], summary: &str) -> String {
        for (path, text) in files {
            fs::write(root.join(path), text).unwrap();
        }
        git(root, &["add", "-A"]);
        let message = format!("{summary}\n\nBody of {summary}.");
        git(root, &["commit", "-q", "--allow-empty", "-m", &message]);
        git(root, &["rev-parse", "HEAD"])
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        repo: Repo,
        c1: String,
        c2: String,
        t1: String,
        m: String,
        s1: String,
    }

    /// main: c1 (a.rs), c2 (edits a.rs, adds n.rs), then m merging topic's t1
    /// (off c1, adds t.rs); s1 off c2 on an unmerged branch side; v1 tags c1;
    /// origin/main is at c2, with origin/HEAD pointing at it.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q"]);
        let c1 = commit(&root, &[("a.rs", "one\ntwo\n")], "C1");
        git(&root, &["tag", "v1"]);
        let c2 = commit(
            &root,
            &[("a.rs", "one\ntwo\nthree\n"), ("n.rs", "new\n")],
            "C2",
        );
        git(&root, &["update-ref", "refs/remotes/origin/main", &c2]);
        git(
            &root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        git(&root, &["checkout", "-q", "-b", "side"]);
        let s1 = commit(&root, &[("s.rs", "side\n")], "S1");
        git(&root, &["checkout", "-q", "-b", "topic", &c1]);
        let t1 = commit(&root, &[("t.rs", "topic\n")], "T1");
        git(&root, &["checkout", "-q", "main"]);
        git(&root, &["merge", "-q", "--no-ff", "-m", "M", "topic"]);
        let m = git(&root, &["rev-parse", "HEAD"]);
        let repo = Repo::discover(&root).unwrap().expect("a repository");
        Fixture {
            _dir: dir,
            root,
            repo,
            c1,
            c2,
            t1,
            m,
            s1,
        }
    }

    /// `(id, parents)` of each commit, as `git log --topo-order` lists `revs`.
    fn git_log(root: &Path, revs: &[&str]) -> Vec<(String, Vec<String>)> {
        let mut args = vec!["log", "--topo-order", "--format=%H %P"];
        args.extend(revs);
        args.push("--");
        git(root, &args)
            .lines()
            .map(|line| {
                let mut ids = line.split_whitespace().map(str::to_owned);
                let id = ids.next().unwrap();
                (id, ids.collect())
            })
            .collect()
    }

    fn ids_and_parents(log: &[LogCommit]) -> Vec<(String, Vec<String>)> {
        log.iter()
            .map(|entry| (entry.commit.id.clone(), entry.parents.clone()))
            .collect()
    }

    fn refs_of<'a>(log: &'a [LogCommit], id: &str) -> &'a [RefLabel] {
        &log.iter().find(|entry| entry.commit.id == id).unwrap().refs
    }

    fn label(name: &str, kind: RefKind, is_head: bool) -> RefLabel {
        RefLabel {
            name: name.to_owned(),
            kind,
            is_head,
        }
    }

    #[test]
    fn head_and_all() {
        let f = fixture();
        let head = f.repo.log(LogScope::Head, 0, 100).unwrap();
        let all = f.repo.log(LogScope::All, 0, 100).unwrap();
        assert_eq!(ids_and_parents(&head), git_log(&f.root, &["HEAD"]));
        assert_eq!(
            ids_and_parents(&all),
            git_log(&f.root, &["--branches", "--remotes", "--tags", "HEAD"])
        );
        assert_eq!(head.len(), 4);
        assert_eq!(all.len(), 5);
        assert!(!head.iter().any(|entry| entry.commit.id == f.s1));
        assert!(all.iter().any(|entry| entry.commit.id == f.s1));
        assert_eq!(head[0].parents, [f.c2.clone(), f.t1.clone()]);

        assert_eq!(refs_of(&all, &f.m), [label("main", RefKind::Branch, true)]);
        assert_eq!(refs_of(&all, &f.c1), [label("v1", RefKind::Tag, false)]);
        assert_eq!(
            refs_of(&all, &f.c2),
            [label("origin/main", RefKind::Remote, false)]
        );
        assert_eq!(
            refs_of(&all, &f.t1),
            [label("topic", RefKind::Branch, false)]
        );
        assert_eq!(
            refs_of(&all, &f.s1),
            [label("side", RefKind::Branch, false)]
        );
        assert_eq!(refs_of(&head, &f.m), refs_of(&all, &f.m));

        git(&f.root, &["checkout", "-q", "--detach", &f.c2]);
        let detached = f.repo.log(LogScope::Head, 0, 100).unwrap();
        assert_eq!(detached[0].commit.id, f.c2);
        assert_eq!(
            detached[0].refs,
            [
                label("HEAD", RefKind::Head, true),
                label("origin/main", RefKind::Remote, false)
            ]
        );
        let all = f.repo.log(LogScope::All, 0, 100).unwrap();
        assert_eq!(refs_of(&all, &f.m), [label("main", RefKind::Branch, false)]);
    }

    #[test]
    fn paging_and_edges() {
        let f = fixture();
        let whole = f.repo.log(LogScope::All, 0, 100).unwrap();
        let mut paged = f.repo.log(LogScope::All, 0, 2).unwrap();
        assert_eq!(paged.len(), 2);
        paged.extend(f.repo.log(LogScope::All, 2, 100).unwrap());
        assert_eq!(paged, whole);
        assert!(f.repo.log(LogScope::All, 0, 0).unwrap().is_empty());
        assert!(f.repo.log(LogScope::Head, 100, 10).unwrap().is_empty());

        let head = f.repo.log(LogScope::Head, 0, 100).unwrap();
        for (key, value) in [
            ("color.ui", "always"),
            ("log.decorate", "short"),
            ("log.showSignature", "true"),
            ("log.abbrevCommit", "true"),
            ("format.pretty", "oneline"),
            ("log.excludeDecoration", "refs/tags/"),
        ] {
            git(&f.root, &["config", key, value]);
        }
        assert_eq!(f.repo.log(LogScope::All, 0, 100).unwrap(), whole);
        assert_eq!(f.repo.log(LogScope::Head, 0, 100).unwrap(), head);

        // Each commit as blame shows it.
        let mut blamed: HashMap<String, CommitInfo> = HashMap::new();
        for path in ["a.rs", "t.rs"] {
            let path = f.root.join(path);
            let text = fs::read_to_string(&path).unwrap();
            for run in f.repo.blame(&path, &text).unwrap() {
                let commit = run.commit.unwrap();
                blamed.insert(commit.id.clone(), (*commit).clone());
            }
        }
        assert_eq!(blamed.len(), 3);
        for entry in &whole {
            if let Some(info) = blamed.remove(&entry.commit.id) {
                assert_eq!(*entry.commit, info);
            }
        }
        assert!(blamed.is_empty(), "every blamed commit is in the log");
        assert_eq!(whole[0].commit.summary, "M");

        git(&f.root, &["checkout", "-q", "--orphan", "unborn"]);
        assert!(f.repo.log(LogScope::Head, 0, 100).unwrap().is_empty());
        assert!(f.repo.log(LogScope::All, 0, 100).unwrap().is_empty());

        let empty = tempfile::tempdir().unwrap();
        let empty_root = empty.path().canonicalize().unwrap();
        git(&empty_root, &["init", "-q"]);
        let repo = Repo::discover(&empty_root).unwrap().expect("a repository");
        assert!(repo.log(LogScope::Head, 0, 100).unwrap().is_empty());
        assert!(repo.log(LogScope::All, 0, 100).unwrap().is_empty());
    }

    #[test]
    fn commit_files() {
        let f = fixture();
        let file = |path: &str, kind, from: Option<&str>| CommitFile {
            path: f.root.join(path),
            from: from.map(|from| f.root.join(from)),
            kind,
        };
        assert_eq!(
            f.repo.commit_files(&f.c2).unwrap(),
            [
                file("a.rs", ChangeKind::Modified, None),
                file("n.rs", ChangeKind::Added, None),
            ]
        );
        assert_eq!(
            f.repo.commit_files(&f.c1).unwrap(),
            [file("a.rs", ChangeKind::Added, None)]
        );
        // Against its first parent, c2, not a combined diff.
        assert_eq!(
            f.repo.commit_files(&f.m).unwrap(),
            [file("t.rs", ChangeKind::Added, None)]
        );
        assert_eq!(
            f.repo.commit_files(&f.c2[..7]).unwrap(),
            f.repo.commit_files(&f.c2).unwrap()
        );

        git(&f.root, &["mv", "a.rs", "b.rs"]);
        let rename = commit(&f.root, &[], "Rename");
        assert_eq!(
            f.repo.commit_files(&rename).unwrap(),
            [file("b.rs", ChangeKind::Modified, Some("a.rs"))]
        );
        git(&f.root, &["rm", "-q", "n.rs"]);
        let delete = commit(&f.root, &[], "Delete");
        assert_eq!(
            f.repo.commit_files(&delete).unwrap(),
            [file("n.rs", ChangeKind::Deleted, None)]
        );

        assert!(f.repo.commit_files(&"0".repeat(40)).is_err());
        assert!(f.repo.commit_files("not-an-id").is_err());
    }
}
