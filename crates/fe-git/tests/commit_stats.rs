//! Per-file line counts of a commit, against temp repositories built with `git`.

mod commit_stats {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use fe_git::Repo;

    fn git(dir: &Path, args: &[&str]) -> String {
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

    fn init() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        git(&root, &["init", "-q"]);
        (dir, root)
    }

    fn commit(root: &Path, files: &[(&str, &[u8])], summary: &str) -> String {
        for (path, bytes) in files {
            fs::write(root.join(path), bytes).unwrap();
        }
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", summary]);
        git(root, &["rev-parse", "HEAD"])
    }

    fn lines(range: std::ops::Range<u32>) -> Vec<u8> {
        range
            .map(|n| format!("line {n}\n"))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn counts() {
        let (_dir, root) = init();
        commit(&root, &[("a.rs", b"one\ntwo\nthree\n")], "base");
        let id = commit(
            &root,
            &[
                ("a.rs", b"ONE\ntwo\nthree\nx\ny\n"),
                ("n.rs", b"1\n2\n3\n4\n5\n"),
            ],
            "edit",
        );
        let stats = Repo::discover(&root)
            .unwrap()
            .unwrap()
            .commit_stats(&id)
            .unwrap();
        let got: Vec<_> = stats
            .files
            .iter()
            .map(|f| (f.path.clone(), f.from.clone(), f.added, f.removed))
            .collect();
        assert_eq!(
            got,
            vec![
                (root.join("a.rs"), None, Some(3), Some(1)),
                (root.join("n.rs"), None, Some(5), Some(0)),
            ]
        );
        assert_eq!(stats.total(), (8, 1));
    }

    #[test]
    fn binary_and_rename() {
        let (_dir, root) = init();
        commit(&root, &[("old.txt", &lines(0..20))], "base");
        git(&root, &["mv", "old.txt", "new.txt"]);
        let mut edited = lines(0..20);
        edited.extend_from_slice(b"extra\n");
        let id = commit(
            &root,
            &[("new.txt", &edited), ("b.bin", &[0, 1, 2, 0, 255])],
            "mv",
        );
        let stats = Repo::discover(&root)
            .unwrap()
            .unwrap()
            .commit_stats(&id)
            .unwrap();
        assert_eq!(stats.files.len(), 2);
        let bin = &stats.files[0];
        assert_eq!(bin.path, root.join("b.bin"));
        assert_eq!((bin.added, bin.removed), (None, None));
        let renamed = &stats.files[1];
        assert_eq!(renamed.path, root.join("new.txt"));
        assert_eq!(renamed.from, Some(root.join("old.txt")));
        assert_eq!((renamed.added, renamed.removed), (Some(1), Some(0)));
        assert_eq!(stats.total(), (1, 0));
    }

    #[test]
    fn root_and_merge() {
        let (_dir, root) = init();
        git(&root, &["config", "log.showRoot", "false"]);
        let first = commit(&root, &[("a.txt", b"1\n2\n")], "root");
        let repo = Repo::discover(&root).unwrap().unwrap();
        assert_eq!(repo.commit_stats(&first).unwrap().total(), (2, 0));

        git(&root, &["checkout", "-q", "-b", "side"]);
        commit(&root, &[("s.txt", b"s1\ns2\ns3\n")], "side");
        git(&root, &["checkout", "-q", "main"]);
        commit(&root, &[("m.txt", b"m\n")], "main");
        git(&root, &["merge", "-q", "--no-ff", "-m", "merge", "side"]);
        let merge = git(&root, &["rev-parse", "HEAD"]);
        let stats = repo.commit_stats(&merge).unwrap();
        assert_eq!(stats.files.len(), 1);
        assert_eq!(stats.files[0].path, root.join("s.txt"));
        assert_eq!(stats.total(), (3, 0));
    }

    #[test]
    fn paths_with_spaces_and_unicode() {
        let (_dir, root) = init();
        commit(&root, &[("base", b"x\n")], "base");
        // Both names git quotes; a tab can't be in a Windows file name.
        #[cfg(unix)]
        let odd = "naïve\ttab é.txt";
        #[cfg(windows)]
        let odd = "naïve tab é.txt";
        let id = commit(
            &root,
            &[("with space.txt", b"a\n"), (odd, b"a\nb\n")],
            "odd names",
        );
        let stats = Repo::discover(&root)
            .unwrap()
            .unwrap()
            .commit_stats(&id)
            .unwrap();
        let paths: Vec<_> = stats.files.iter().map(|f| f.path.clone()).collect();
        assert!(paths.contains(&root.join("with space.txt")));
        assert!(paths.contains(&root.join(odd)));
        assert_eq!(stats.total(), (3, 0));
    }
}
