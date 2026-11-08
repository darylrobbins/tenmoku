//! A file's commit list, against temp repositories built with the `git` CLI.

mod file_log {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

    use fe_git::{CommitInfo, GitError, Repo};

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

    /// Writes `text` to `path`, commits everything and returns the id.
    fn commit(root: &Path, path: &str, text: &str, summary: &str) -> String {
        fs::write(root.join(path), text).unwrap();
        git(root, &["add", "-A"]);
        git(
            root,
            &[
                "commit",
                "-q",
                "-m",
                &format!("{summary}\n\nBody of {summary}."),
            ],
        );
        git(root, &["rev-parse", "HEAD"])
    }

    const BODY: &str = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\n";

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        repo: Repo,
        /// c1..c4.
        ids: [String; 4],
    }

    /// a.rs (c1), edited (c2), c.rs, git mv'd to b.rs with an edit (c3), b.rs edited (c4).
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        git(&root, &["init", "-q"]);
        let c1 = commit(&root, "a.rs", BODY, "C1");
        let c2 = commit(&root, "a.rs", &format!("{BODY}iota\n"), "C2");
        commit(&root, "c.rs", "unrelated\n", "Other");
        git(&root, &["mv", "a.rs", "b.rs"]);
        let c3 = commit(&root, "b.rs", &format!("{BODY}iota\nkappa\n"), "C3");
        let c4 = commit(&root, "b.rs", &format!("{BODY}iota\nkappa\nlambda\n"), "C4");
        let repo = Repo::discover(&root).unwrap().expect("a repository");
        Fixture {
            _dir: dir,
            root,
            repo,
            ids: [c1, c2, c3, c4],
        }
    }

    #[test]
    fn follows_renames() {
        let f = fixture();
        let log = f.repo.file_log(&f.root.join("b.rs"), 100).unwrap();
        let [c1, c2, c3, c4] = &f.ids;
        let ids: Vec<&str> = log.iter().map(|entry| entry.commit.id.as_str()).collect();
        assert_eq!(ids, [c4, c3, c2, c1]);
        let (a, b) = (f.root.join("a.rs"), f.root.join("b.rs"));
        let paths: Vec<&PathBuf> = log.iter().map(|entry| &entry.path).collect();
        assert_eq!(paths, [&b, &b, &a, &a]);
        let from: Vec<Option<&PathBuf>> = log.iter().map(|entry| entry.from.as_ref()).collect();
        assert_eq!(from, [None, Some(&a), None, None]);
    }

    #[test]
    fn limits_and_empty() {
        let f = fixture();
        let b = f.root.join("b.rs");
        let log = f.repo.file_log(&b, 2).unwrap();
        let ids: Vec<&str> = log.iter().map(|entry| entry.commit.id.as_str()).collect();
        assert_eq!(ids, [&f.ids[3], &f.ids[2]]);

        assert!(f.repo.file_log(&b, 0).unwrap().is_empty());
        fs::write(f.root.join("new.rs"), "x\n").unwrap();
        assert!(
            f.repo
                .file_log(&f.root.join("new.rs"), 10)
                .unwrap()
                .is_empty()
        );

        let unborn = tempfile::tempdir().unwrap();
        let unborn_root = dunce::canonicalize(unborn.path()).unwrap();
        git(&unborn_root, &["init", "-q"]);
        let repo = Repo::discover(&unborn_root).unwrap().expect("a repository");
        assert!(
            repo.file_log(&unborn_root.join("a.rs"), 10)
                .unwrap()
                .is_empty()
        );

        let outside = std::env::temp_dir().join("elsewhere.rs");
        assert!(matches!(
            f.repo.file_log(&outside, 10),
            Err(GitError::OutsideWorktree(_))
        ));
    }

    #[test]
    fn matches_blame_and_ignores_config() {
        let f = fixture();
        let b = f.root.join("b.rs");
        let plain = f.repo.file_log(&b, 100).unwrap();
        for (key, value) in [
            ("color.ui", "always"),
            ("log.decorate", "full"),
            ("log.showSignature", "true"),
            ("log.follow", "false"),
            ("diff.renames", "false"),
            ("format.pretty", "oneline"),
        ] {
            git(&f.root, &["config", key, value]);
        }
        let log = f.repo.file_log(&b, 100).unwrap();
        assert_eq!(log, plain);
        assert_eq!(log.len(), 4);

        let text = fs::read_to_string(&b).unwrap();
        let runs = f.repo.blame(&b, &text).unwrap();
        for entry in &log {
            let blamed: Vec<&Arc<CommitInfo>> = runs
                .iter()
                .filter_map(|run| run.commit.as_ref())
                .filter(|commit| commit.id == entry.commit.id)
                .collect();
            assert!(!blamed.is_empty(), "blame has rows of {}", entry.commit.id);
            assert_eq!(&**blamed[0], &*entry.commit);
        }
    }

    /// A file renamed without changes keeps its rows' original commits, as in
    /// `git blame`.
    #[test]
    fn blame_follows_a_pure_rename() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        git(&root, &["init", "-q"]);
        let first = commit(&root, "a.rs", BODY, "First");
        git(&root, &["mv", "a.rs", "b.rs"]);
        let rename = commit(&root, "b.rs", BODY, "Rename");
        let repo = Repo::discover(&root).unwrap().expect("a repository");
        let runs = repo.blame(&root.join("b.rs"), BODY).unwrap();
        assert_eq!(runs.len(), 1);
        let id = &runs[0].commit.as_ref().unwrap().id;
        assert_eq!(id, &first);
        assert_ne!(id, &rename);
    }

    /// `.` components and doubled separators in the path don't break blame.
    #[test]
    fn blame_normalizes_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        git(&root, &["init", "-q"]);
        fs::create_dir(root.join("dir")).unwrap();
        let id = commit(&root, "dir/a.rs", BODY, "First");
        let repo = Repo::discover(&root).unwrap().expect("a repository");
        let plain = repo.blame(&root.join("dir/a.rs"), BODY).unwrap();
        assert_eq!(plain[0].commit.as_ref().unwrap().id, id);
        let odd = PathBuf::from(format!("{}/dir/./a.rs", root.display()));
        assert_eq!(repo.blame(&odd, BODY).unwrap(), plain);
        let doubled = PathBuf::from(format!("{}//dir//a.rs", root.display()));
        assert_eq!(repo.blame(&doubled, BODY).unwrap(), plain);
    }
}
