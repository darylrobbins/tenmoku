//! Paging a commit log from its tips, fixed when the first page is read, so
//! commits, resets and branch changes between pages don't shift it, against
//! temp repositories built with the `git` CLI.

mod commit_log_tips {
    use std::path::Path;
    use std::process::Command;

    use fe_git::{LogScope, Repo};

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
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

    fn commit(root: &Path, summary: &str) -> String {
        git(root, &["commit", "-q", "--allow-empty", "-m", summary]);
        git(root, &["rev-parse", "HEAD"])
    }

    /// `main` with four commits, an annotated tag on the first, and `side`
    /// off the second with one of its own, unmerged.
    fn fixture() -> (tempfile::TempDir, Repo) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        commit(root, "c1");
        git(root, &["tag", "-a", "v1", "-m", "v1"]);
        commit(root, "c2");
        git(root, &["checkout", "-q", "-b", "side"]);
        commit(root, "s1");
        git(root, &["checkout", "-q", "main"]);
        commit(root, "c3");
        commit(root, "c4");
        let repo = Repo::discover(root).unwrap().unwrap();
        (dir, repo)
    }

    fn ids(commits: &[fe_git::LogCommit]) -> Vec<String> {
        (commits.iter())
            .map(|commit| commit.commit.id.clone())
            .collect()
    }

    #[test]
    fn tips_are_the_refs_now() {
        let (dir, repo) = fixture();
        let root = dir.path();
        let head = git(root, &["rev-parse", "HEAD"]);
        assert_eq!(
            repo.log_tips(LogScope::Head).unwrap(),
            std::slice::from_ref(&head)
        );

        let tips = repo.log_tips(LogScope::All).unwrap();
        let side = git(root, &["rev-parse", "side"]);
        let tag = git(root, &["rev-parse", "v1"]);
        // HEAD first; main, the same commit, only once.
        assert_eq!(tips, [head, side, tag]);
    }

    #[test]
    fn logs_from_tips_as_from_refs() {
        let (_dir, repo) = fixture();
        for scope in [LogScope::Head, LogScope::All] {
            let tips = repo.log_tips(scope).unwrap();
            let from_refs = repo.log(scope, 0, 100).unwrap();
            assert_eq!(repo.log_from(&tips, 0, 100).unwrap(), from_refs);
            assert_eq!(repo.log_from(&tips, 1, 2).unwrap(), from_refs[1..3]);
            assert_eq!(repo.log_from(&tips, 0, 0).unwrap(), []);
        }
        assert_eq!(repo.log_from(&[], 0, 100).unwrap(), []);
    }

    #[test]
    fn pages_stay_on_their_history_as_refs_move() {
        let (dir, repo) = fixture();
        let root = dir.path();
        for scope in [LogScope::Head, LogScope::All] {
            let tips = repo.log_tips(scope).unwrap();
            let whole = ids(&repo.log(scope, 0, 100).unwrap());
            let first = ids(&repo.log_from(&tips, 0, 2).unwrap());
            assert_eq!(first, whole[..2]);

            // A commit, a reset and a deleted branch between pages.
            let (before, side) = (
                git(root, &["rev-parse", "HEAD"]),
                git(root, &["rev-parse", "side"]),
            );
            commit(root, "between");
            git(root, &["reset", "-q", "--hard", "HEAD~3"]);
            git(root, &["branch", "-q", "-D", "side"]);
            assert_ne!(ids(&repo.log(scope, 0, 100).unwrap()), whole);

            let rest = ids(&repo.log_from(&tips, 2, 100).unwrap());
            assert_eq!([first, rest].concat(), whole, "{scope:?}");

            // Put back for the next scope.
            git(root, &["reset", "-q", "--hard", &before]);
            git(root, &["branch", "-q", "side", &side]);
        }
    }

    #[test]
    fn pages_go_on_after_switching_to_an_unborn_branch() {
        let (dir, repo) = fixture();
        let root = dir.path();
        for scope in [LogScope::Head, LogScope::All] {
            let tips = repo.log_tips(scope).unwrap();
            let whole = ids(&repo.log(scope, 0, 100).unwrap());
            let first = ids(&repo.log_from(&tips, 0, 2).unwrap());

            git(root, &["checkout", "-q", "--orphan", "unborn"]);
            assert_eq!(repo.log(scope, 0, 100).unwrap(), []);
            let rest = ids(&repo.log_from(&tips, 2, 100).unwrap());
            assert_eq!([first, rest].concat(), whole, "{scope:?}");

            git(root, &["checkout", "-q", "-f", "main"]);
        }
    }

    #[test]
    fn an_unborn_head_has_no_tips() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        let repo = Repo::discover(dir.path()).unwrap().unwrap();
        assert_eq!(repo.log_tips(LogScope::All).unwrap(), Vec::<String>::new());
    }
}
