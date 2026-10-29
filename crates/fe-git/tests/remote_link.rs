//! Remote URL to commit page mapping.

mod remote_link {
    use std::path::Path;
    use std::process::Command;

    use fe_git::Repo;
    use fe_git::remote_link::{
        DEFAULT_PROVIDERS, ParsedRemote, RemoteLinkProvider, commit_url, commit_url_with,
    };

    #[test]
    fn github_forms() {
        for url in [
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "ssh://git@github.com/o/r.git",
            "ssh://git@github.com:22/o/r.git",
            "git@github.com:o/r.git",
        ] {
            assert_eq!(
                commit_url(url, "abc123").as_deref(),
                Some("https://github.com/o/r/commit/abc123"),
                "{url}"
            );
        }
    }

    #[test]
    fn no_match() {
        for url in [
            "https://gitlab.com/o/r.git",
            "git@gitlab.com:o/r.git",
            "/srv/git/r.git",
            "../r",
            "",
            "https://notgithub.com/o/r",
            "https://github.com.evil.io/o/r",
            "git@github.com.evil.io:o/r.git",
            "https://github.com/o",
        ] {
            assert_eq!(commit_url(url, "abc"), None, "{url}");
        }
    }

    struct Example;
    impl RemoteLinkProvider for Example {
        fn matches(&self, remote: &ParsedRemote) -> bool {
            remote.host == "git.example.org"
        }
        fn commit_url(&self, remote: &ParsedRemote, id: &str) -> String {
            format!("https://git.example.org/{}/-/c/{id}", remote.path)
        }
    }

    #[test]
    fn seam() {
        assert_eq!(DEFAULT_PROVIDERS.len(), 1);
        let providers: [&dyn RemoteLinkProvider; 2] = [DEFAULT_PROVIDERS[0], &Example];
        assert_eq!(
            commit_url_with(&providers, "git@git.example.org:a/b.git", "1"),
            Some("https://git.example.org/a/b/-/c/1".into())
        );
        assert_eq!(
            commit_url_with(&providers, "https://github.com/o/r", "1"),
            Some("https://github.com/o/r/commit/1".into())
        );
        assert_eq!(commit_url("git@git.example.org:a/b.git", "1"), None);
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    }

    fn repo_with(remotes: &[(&str, &str)]) -> (tempfile::TempDir, Repo) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        for (name, url) in remotes {
            git(dir.path(), &["remote", "add", name, url]);
        }
        let repo = Repo::discover(dir.path()).unwrap().unwrap();
        (dir, repo)
    }

    #[test]
    fn which_remote() {
        let (_d, repo) = repo_with(&[
            ("up", "https://github.com/up/r"),
            ("origin", "git@github.com:o/r.git"),
        ]);
        assert_eq!(
            repo.commit_web_url("c1").as_deref(),
            Some("https://github.com/o/r/commit/c1")
        );

        let (_d, repo) = repo_with(&[("fork", "https://github.com/f/r")]);
        assert_eq!(
            repo.commit_web_url("c1").as_deref(),
            Some("https://github.com/f/r/commit/c1")
        );

        let (_d, repo) = repo_with(&[
            ("a", "https://github.com/a/r"),
            ("b", "https://github.com/b/r"),
        ]);
        assert_eq!(repo.commit_web_url("c1"), None);

        let (_d, repo) = repo_with(&[]);
        assert_eq!(repo.commit_web_url("c1"), None);

        let (d, repo) = repo_with(&[("origin", "https://github.com/o/r")]);
        git(
            d.path(),
            &["remote", "set-url", "origin", "https://github.com/n/r"],
        );
        assert_eq!(
            repo.commit_web_url("c1").as_deref(),
            Some("https://github.com/n/r/commit/c1")
        );
        git(d.path(), &["remote", "rename", "origin", "other"]);
        git(
            d.path(),
            &["remote", "add", "second", "https://github.com/s/r"],
        );
        assert_eq!(repo.commit_web_url("c1"), None);
    }
}
