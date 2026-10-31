//! A repository's commit log narrowed to the commits a filter matches, by
//! message, author or id, against temp repositories built with the `git`
//! CLI.

mod commit_filter {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use fe_git::{LogCommit, LogFilter, LogScope, Repo};

    fn git(dir: &Path, author: (&str, &str), args: &[&str]) -> String {
        let date = "1700000000 +0100";
        let output = Command::new("git")
            .args(["-c", &format!("user.name={}", author.0)])
            .args(["-c", &format!("user.email={}", author.1)])
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

    const ADA: (&str, &str) = ("Ada Lovelace", "ada@example.com");
    const GRACE: (&str, &str) = ("Grace Hopper", "grace@navy.example");

    /// An empty commit by `author` with `message`; its id.
    fn commit(root: &Path, author: (&str, &str), message: &str) -> String {
        git(
            root,
            author,
            &["commit", "-q", "--allow-empty", "-m", message],
        );
        git(root, author, &["rev-parse", "HEAD"])
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        repo: Repo,
        /// Each commit's id, by its subject.
        ids: Vec<(&'static str, String)>,
    }

    impl Fixture {
        fn id(&self, subject: &str) -> &str {
            let found = self.ids.iter().find(|(name, _)| *name == subject);
            &found.expect("a commit of the fixture").1
        }
    }

    /// main: `Add parser` (Ada, its body naming the lexer), `Fix the
    /// Parser crash` (Grace), `Docs` (Ada, 1700 in its body), `Tidy`
    /// (Grace); `side` off `Add parser`: `Parser on side` (Grace).
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root: PathBuf = dir.path().canonicalize().unwrap();
        git(&root, ADA, &["init", "-q"]);
        let add = commit(&root, ADA, "Add parser\n\nUses the new LEXER.");
        let fix = commit(&root, GRACE, "Fix the Parser crash");
        let docs = commit(&root, ADA, "Docs\n\nSince 1700.");
        let tidy = commit(&root, GRACE, "Tidy");
        git(&root, ADA, &["checkout", "-q", "-b", "side", &add]);
        let side = commit(&root, GRACE, "Parser on side");
        git(&root, ADA, &["checkout", "-q", "main"]);
        let repo = Repo::discover(&root).unwrap().expect("a repository");
        Fixture {
            _dir: dir,
            repo,
            ids: vec![
                ("Add parser", add),
                ("Fix the Parser crash", fix),
                ("Docs", docs),
                ("Tidy", tidy),
                ("Parser on side", side),
            ],
        }
    }

    fn summaries(commits: &[LogCommit]) -> Vec<String> {
        (commits.iter())
            .map(|entry| entry.commit.summary.clone())
            .collect()
    }

    /// The subjects of `scope`'s log filtered by `text`.
    fn matching(f: &Fixture, scope: LogScope, text: &str) -> Vec<String> {
        let filter = LogFilter::new(text);
        summaries(&f.repo.log_matching(scope, &filter, 0, 100).unwrap())
    }

    /// `subjects` in the order `scope`'s unfiltered log lists them.
    fn in_order(f: &Fixture, scope: LogScope, subjects: &[&str]) -> Vec<String> {
        let whole = summaries(&f.repo.log(scope, 0, 100).unwrap());
        let listed: Vec<String> = (whole.into_iter())
            .filter(|subject| subjects.contains(&subject.as_str()))
            .collect();
        assert_eq!(listed.len(), subjects.len(), "{subjects:?} are all listed");
        listed
    }

    #[test]
    fn by_text() {
        let f = fixture();
        let all = LogScope::All;
        // In the subject, any case, in log order.
        assert_eq!(
            matching(&f, all, "pArSeR"),
            in_order(
                &f,
                all,
                &["Parser on side", "Fix the Parser crash", "Add parser"]
            )
        );
        // In the body.
        assert_eq!(
            matching(&f, all, "lexer"),
            in_order(&f, all, &["Add parser"])
        );
        // Every word, anywhere in the message, in any order.
        assert_eq!(
            matching(&f, all, "  crash   fix "),
            in_order(&f, all, &["Fix the Parser crash"])
        );
        assert!(matching(&f, all, "crash lexer").is_empty());
        // By the author's name or email, whatever the message says.
        assert_eq!(
            matching(&f, all, "lovelace"),
            in_order(&f, all, &["Docs", "Add parser"])
        );
        assert_eq!(
            matching(&f, all, "NAVY.example"),
            in_order(&f, all, &["Parser on side", "Tidy", "Fix the Parser crash"])
        );
        assert_eq!(matching(&f, all, "grace hopper").len(), 3);
        // A hex prefix of 4 or more digits, any case, matches an id.
        let tidy = f.id("Tidy").to_owned();
        assert_eq!(matching(&f, all, &tidy[..4].to_uppercase()), ["Tidy"]);
        assert_eq!(matching(&f, all, &tidy), ["Tidy"]);
        // Fewer than 4 digits are text alone, not an id's prefix.
        let short = &tidy[..3];
        let whole = f.repo.log(LogScope::All, 0, 100).unwrap();
        let as_text: Vec<String> = (whole.iter())
            .map(|entry| &entry.commit)
            .filter(|commit| {
                [&commit.message, &commit.author, &commit.email]
                    .iter()
                    .any(|field| field.to_lowercase().contains(short))
            })
            .map(|commit| commit.summary.clone())
            .collect();
        assert_eq!(matching(&f, all, short), as_text);
        // Digits that are no id's prefix still match a message, and not the
        // author's date.
        assert_eq!(matching(&f, all, "1700"), in_order(&f, all, &["Docs"]));
        // Nothing matches; the scope narrows as unfiltered.
        assert!(matching(&f, all, "no such words").is_empty());
        assert_eq!(
            matching(&f, LogScope::Head, "parser"),
            in_order(&f, LogScope::Head, &["Fix the Parser crash", "Add parser"])
        );
        // An empty filter, or only whitespace, keeps every commit.
        for text in ["", "  \t"] {
            let filter = LogFilter::new(text);
            assert!(filter.is_empty());
            assert_eq!(
                f.repo.log_matching(LogScope::All, &filter, 0, 100).unwrap(),
                whole
            );
        }
        // Each commit listed is the unfiltered log's, refs and parents too.
        let filtered = (f.repo)
            .log_matching(all, &LogFilter::new("parser"), 0, 100)
            .unwrap();
        for entry in &filtered {
            assert!(whole.contains(entry), "{entry:?} is as unfiltered");
        }
        // `matches` is the same test, commit by commit.
        let filter = LogFilter::new("PARSER");
        let kept: Vec<_> = (whole.iter())
            .filter(|entry| filter.matches(&entry.commit))
            .cloned()
            .collect();
        assert_eq!(kept, filtered);
    }

    #[test]
    fn paging() {
        let f = fixture();
        for (scope, text) in [
            (LogScope::All, "parser"),
            (LogScope::All, "grace"),
            (LogScope::Head, "a"),
        ] {
            let filter = LogFilter::new(text);
            let whole = f.repo.log_matching(scope, &filter, 0, 100).unwrap();
            assert!(whole.len() >= 3, "{text}: {}", whole.len());
            let mut paged = f.repo.log_matching(scope, &filter, 0, 2).unwrap();
            assert_eq!(paged.len(), 2);
            paged.extend(f.repo.log_matching(scope, &filter, 2, 100).unwrap());
            assert_eq!(paged, whole, "{text}");
            // A page from the tips reads the same.
            let tips = f.repo.log_tips(scope).unwrap();
            let mut from = f.repo.log_from_matching(&tips, &filter, 0, 1).unwrap();
            from.extend(f.repo.log_from_matching(&tips, &filter, 1, 1).unwrap());
            from.extend(f.repo.log_from_matching(&tips, &filter, 2, 100).unwrap());
            assert_eq!(from, whole, "{text} from the tips");
            // Past the matches, or no room: nothing.
            let past = whole.len();
            assert!(
                f.repo
                    .log_matching(scope, &filter, past, 10)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                f.repo
                    .log_matching(scope, &filter, 0, 0)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                f.repo
                    .log_from_matching(&[], &filter, 0, 10)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
