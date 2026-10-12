//! Blame and texts at commits, against temp repositories with commits by
//! different authors at different times.

mod blame {
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

    use fe_git::{BlameRun, Repo};

    fn git_as(dir: &Path, author: Option<(&str, &str, &str)>, args: &[&str]) -> String {
        let (name, email, date) = author.unwrap_or(("t", "t@t", "1700000000 +0000"));
        let output = Command::new("git")
            .args(["-c", &format!("user.name={name}")])
            .args(["-c", &format!("user.email={email}")])
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

    fn git(dir: &Path, args: &[&str]) -> String {
        git_as(dir, None, args)
    }

    /// Writes `path`, commits it as `author` at `date`, and returns the commit id.
    fn commit(
        root: &Path,
        path: &str,
        text: &str,
        author: (&str, &str, &str),
        summary: &str,
    ) -> String {
        fs::write(root.join(path), text).unwrap();
        git(root, &["add", path]);
        let message = format!("{summary}\n\nBody of {summary}.");
        git_as(root, Some(author), &["commit", "-q", "-m", &message]);
        git(root, &["rev-parse", "HEAD"])
    }

    const ADA: (&str, &str, &str) = ("Ada", "ada@example.com", "1700000000 +0130");
    const BOB: (&str, &str, &str) = ("Bob", "bob@example.com", "1700100000 -0500");
    const CY: (&str, &str, &str) = ("Cy", "cy@example.com", "1700200000 +0000");

    /// `a.txt` from three commits by Ada, Bob and Cy; returns their ids.
    fn three_commits() -> (tempfile::TempDir, PathBuf, Repo, [String; 3]) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        git(&root, &["init", "-q"]);
        let first = commit(
            &root,
            "a.txt",
            "one\ntwo\nthree\nfour\nfive\n",
            ADA,
            "First",
        );
        let second = commit(
            &root,
            "a.txt",
            "one\nTWO\nthree\nfour\nfive\nsix\n",
            BOB,
            "Second",
        );
        let third = commit(
            &root,
            "a.txt",
            "zero\none\nTWO\nthree\nFOUR\nfive\nsix\n",
            CY,
            "Third",
        );
        let repo = Repo::discover(&root).unwrap().expect("a repository");
        (dir, root, repo, [first, second, third])
    }

    /// What `git blame --porcelain` says of each row.
    #[derive(Debug, Default, Clone, PartialEq)]
    struct Porcelain {
        id: String,
        author: String,
        email: String,
        time: i64,
        summary: String,
    }

    fn porcelain(root: &Path, path: &str) -> Vec<Porcelain> {
        let out = git(root, &["blame", "--porcelain", path]);
        let mut commits: HashMap<String, Porcelain> = HashMap::new();
        let mut rows = Vec::new();
        let mut current = String::new();
        for line in out.lines() {
            if line.starts_with('\t') {
                rows.push(commits[&current].clone());
                continue;
            }
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            if key.len() == 40 && key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                current = key.to_owned();
                commits.entry(current.clone()).or_insert_with(|| Porcelain {
                    id: current.clone(),
                    ..Porcelain::default()
                });
                continue;
            }
            let entry = commits.get_mut(&current).unwrap();
            match key {
                "author" => entry.author = value.to_owned(),
                "author-mail" => entry.email = value.trim_matches(['<', '>']).to_owned(),
                "author-time" => entry.time = value.parse().unwrap(),
                "summary" => entry.summary = value.to_owned(),
                _ => {}
            }
        }
        rows
    }

    /// Each row's commit id (`None` uncommitted), checking the runs cover
    /// rows `0..` once, in order, without empty runs.
    fn row_ids(runs: &[BlameRun]) -> Vec<Option<String>> {
        let mut ids = Vec::new();
        for run in runs {
            assert_eq!(run.rows.start as usize, ids.len(), "runs {runs:?}");
            assert!(!run.rows.is_empty(), "runs {runs:?}");
            for _ in run.rows.clone() {
                ids.push(run.commit.as_ref().map(|commit| commit.id.clone()));
            }
        }
        ids
    }

    #[test]
    fn runs() {
        let (_dir, root, repo, ids) = three_commits();
        let path = root.join("a.txt");
        let text = fs::read_to_string(&path).unwrap();
        let runs = repo.blame(&path, &text).unwrap();
        let expected = porcelain(&root, "a.txt");
        assert_eq!(expected.len(), 7);

        let mut rows = Vec::new();
        for run in &runs {
            assert_eq!(run.rows.start as usize, rows.len(), "runs {runs:?}");
            assert!(!run.rows.is_empty());
            let commit = run.commit.as_ref().expect("committed");
            for _ in run.rows.clone() {
                rows.push(Porcelain {
                    id: commit.id.clone(),
                    author: commit.author.clone(),
                    email: commit.email.clone(),
                    time: commit.time,
                    summary: commit.summary.clone(),
                });
            }
        }
        assert_eq!(rows, expected);

        let row_commit = |row: usize| rows[row].id.clone();
        assert_eq!(
            (0..7).map(row_commit).collect::<Vec<_>>(),
            [
                &ids[2], &ids[0], &ids[1], &ids[0], &ids[2], &ids[0], &ids[1]
            ]
            .map(String::clone)
        );

        // Every field of one commit, and one `Arc` per commit.
        let bob = runs
            .iter()
            .filter_map(|run| run.commit.clone())
            .filter(|commit| commit.author == "Bob")
            .collect::<Vec<_>>();
        assert_eq!(bob.len(), 2);
        assert!(Arc::ptr_eq(&bob[0], &bob[1]));
        let bob = &bob[0];
        assert_eq!(bob.id, ids[1]);
        assert_eq!(bob.short, ids[1][..7]);
        assert_eq!(bob.email, "bob@example.com");
        assert_eq!(bob.time, 1700100000);
        assert_eq!(bob.offset_minutes, -300);
        assert_eq!(bob.summary, "Second");
        assert_eq!(bob.message.trim_end(), "Second\n\nBody of Second.");
        let ada = runs[1].commit.as_ref().unwrap();
        assert_eq!(ada.offset_minutes, 90);
    }

    #[test]
    fn worktree_text() {
        let (_dir, root, repo, ids) = three_commits();
        let path = root.join("a.txt");
        // HEAD: zero one TWO three FOUR five six. Edit `three`, insert a row
        // after `FOUR`.
        let text = "zero\none\nTWO\nthree!\nFOUR\nnew\nfive\nsix\n";
        let [first, second, third] = ids.map(Some);
        assert_eq!(
            row_ids(&repo.blame(&path, text).unwrap()),
            [
                third.clone(),
                first.clone(),
                second.clone(),
                None,
                third,
                None,
                first,
                second
            ]
        );

        // The text unsaved without a final line break blames the same rows.
        let committed = fs::read_to_string(&path).unwrap();
        assert_eq!(
            row_ids(&repo.blame(&path, committed.trim_end()).unwrap()),
            row_ids(&repo.blame(&path, &committed).unwrap()),
        );

        // A file not in HEAD is one uncommitted run.
        let new = root.join("new.txt");
        let runs = repo.blame(&new, "a\nb\nc\n").unwrap();
        assert_eq!(
            runs,
            [BlameRun {
                rows: 0..3,
                commit: None
            }]
        );
        assert_eq!(repo.blame(&new, "").unwrap(), []);

        // Binary: nothing.
        assert_eq!(repo.blame(&path, "a\0b\n").unwrap(), []);
    }

    #[test]
    fn unborn_head() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        let repo = Repo::discover(root).unwrap().expect("a repository");
        let runs = repo.blame(&root.join("a.txt"), "a\nb").unwrap();
        assert_eq!(
            runs,
            [BlameRun {
                rows: 0..2,
                commit: None
            }]
        );
    }

    #[test]
    fn commit_texts() {
        let (_dir, root, repo, ids) = three_commits();
        let path = root.join("a.txt");
        let b = root.join("b.txt");
        fs::write(&b, "bee\n").unwrap();
        git(&root, &["add", "b.txt"]);
        git(&root, &["commit", "-q", "-m", "Add b"]);
        let fourth = git(&root, &["rev-parse", "HEAD"]);

        assert_eq!(
            repo.text_at(&ids[0], &path).unwrap().as_deref(),
            Some("one\ntwo\nthree\nfour\nfive\n")
        );
        assert_eq!(
            repo.text_at(&ids[1], &path).unwrap().as_deref(),
            Some("one\nTWO\nthree\nfour\nfive\nsix\n")
        );
        assert_eq!(repo.text_at(&ids[2], &b).unwrap(), None);
        assert_eq!(repo.text_at(&fourth, &b).unwrap().as_deref(), Some("bee\n"));
        assert_eq!(
            repo.text_at(&ids[0], &root.join("missing.txt")).unwrap(),
            None
        );

        assert_eq!(repo.parent(&ids[0]).unwrap(), None);
        assert_eq!(
            repo.parent(&ids[1]).unwrap().as_deref(),
            Some(ids[0].as_str())
        );
        assert_eq!(
            repo.parent(&fourth).unwrap().as_deref(),
            Some(ids[2].as_str())
        );
        assert!(repo.parent("not-a-commit").is_err());
    }

    #[test]
    fn first_parent_of_a_merge() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        commit(root, "a.txt", "a\n", ADA, "Base");
        git(root, &["checkout", "-q", "-b", "side"]);
        commit(root, "b.txt", "b\n", BOB, "Side");
        git(root, &["checkout", "-q", "main"]);
        let main = commit(root, "c.txt", "c\n", CY, "Main");
        git(root, &["merge", "-q", "--no-edit", "side"]);
        let merge = git(root, &["rev-parse", "HEAD"]);
        let repo = Repo::discover(root).unwrap().expect("a repository");
        assert_eq!(repo.parent(&merge).unwrap(), Some(main));
    }
}
