//! Merging a branch into HEAD's, in real repositories: `main` and `feat`
//! both change file `f` from a common commit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{BranchRef, MergeError, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

fn rev(root: &Path, name: &str) -> String {
    git(root, &["rev-parse", name])
}

fn subject(root: &Path) -> String {
    git(root, &["log", "-1", "--format=%s"])
}

fn merge_in_progress(root: &Path) -> bool {
    root.join(".git/MERGE_HEAD").exists()
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn local(name: &str) -> BranchRef {
    BranchRef::Local(name.into())
}

/// A repo on `main`, with its own identity and signing off in its config (the
/// library's git reads the user's global config), and `feat` from the
/// first commit. With `conflict`, both change `f`; else `feat` adds `g`.
fn init(conflict: bool) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgSign", "false"),
    ] {
        git(&root, &["config", key, value]);
    }
    commit(&root, "f", "one\n");
    git(&root, &["branch", "feat"]);
    commit(&root, "f", "main\n");
    git(&root, &["checkout", "-q", "feat"]);
    if conflict {
        commit(&root, "f", "feat\n");
    } else {
        commit(&root, "g", "g\n");
    }
    git(&root, &["checkout", "-q", "main"]);
    (dir, root)
}

#[test]
fn merge_conflicts_leave_merge_in_progress() {
    let (_dir, root) = init(true);
    let repo = open(&root);

    match repo.merge_into_current(&local("feat")) {
        Err(MergeError::Conflicts { paths }) => {
            assert_eq!(paths, vec![repo.workdir().join("f")]);
        }
        other => panic!("expected Conflicts, got {other:?}"),
    }
    assert!(merge_in_progress(&root));
    assert_eq!(rev(&root, "MERGE_HEAD"), rev(&root, "feat"));
    assert!(
        fs::read_to_string(root.join("f"))
            .unwrap()
            .contains("<<<<<<<")
    );

    // Another merge is refused by git, and isn't taken for new conflicts.
    match repo.merge_into_current(&local("feat")) {
        Err(MergeError::Failed { output }) => assert!(!output.is_empty()),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(merge_in_progress(&root));
}

#[test]
fn merge_makes_a_commit_with_gits_message() {
    let (_dir, root) = init(false);
    let repo = open(&root);
    let main = rev(&root, "main");

    repo.merge_into_current(&local("feat")).unwrap();
    assert_eq!(subject(&root), "Merge branch 'feat'");
    assert_eq!(rev(&root, "HEAD^1"), main);
    assert_eq!(rev(&root, "HEAD^2"), rev(&root, "feat"));
    assert!(!merge_in_progress(&root));

    // Up to date already, nothing to do.
    let merged = rev(&root, "HEAD");
    repo.merge_into_current(&local("feat")).unwrap();
    assert_eq!(rev(&root, "HEAD"), merged);
}

#[test]
fn merge_a_remote_tracking_branch() {
    let (_dir, root) = init(false);
    let repo = open(&root);
    git(&root, &["remote", "add", "origin", "/nonexistent"]);
    git(&root, &["update-ref", "refs/remotes/origin/feat", "feat"]);
    git(&root, &["branch", "-q", "-f", "feat", "main"]);

    repo.merge_into_current(&BranchRef::Remote {
        remote: "origin".into(),
        branch: "feat".into(),
    })
    .unwrap();
    assert_eq!(subject(&root), "Merge remote-tracking branch 'origin/feat'");
    assert_eq!(rev(&root, "HEAD^2"), rev(&root, "origin/feat"));
}

#[test]
fn merge_the_branch_meant_when_a_tag_shares_its_name() {
    let (_dir, root) = init(false);
    let repo = open(&root);
    // A tag `feat` at the first commit would win over the branch for git.
    git(&root, &["tag", "feat", "main~1"]);

    repo.merge_into_current(&local("feat")).unwrap();
    assert_eq!(rev(&root, "HEAD^2"), rev(&root, "refs/heads/feat"));
}

#[test]
fn merge_fast_forwards_when_it_can() {
    let (_dir, root) = init(false);
    let repo = open(&root);
    git(&root, &["checkout", "-q", "-b", "behind", "main~1"]);

    repo.merge_into_current(&local("main")).unwrap();
    assert_eq!(rev(&root, "HEAD"), rev(&root, "main"));
}

#[test]
fn merge_that_would_sign_is_refused_unless_it_fast_forwards() {
    let (_dir, root) = init(false);
    let repo = open(&root);
    git(&root, &["config", "commit.gpgSign", "true"]);
    let main = rev(&root, "main");

    match repo.merge_into_current(&local("feat")) {
        Err(MergeError::Failed { output }) => {
            assert_eq!(output, kiln_l10n::tr!("git-signing-commit").to_string());
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(rev(&root, "HEAD"), main);
    assert!(!merge_in_progress(&root));

    // mergeOptions sign too.
    git(&root, &["config", "commit.gpgSign", "false"]);
    git(&root, &["config", "branch.main.mergeOptions", "-S"]);
    assert!(matches!(
        repo.merge_into_current(&local("feat")),
        Err(MergeError::Failed { .. })
    ));
    assert_eq!(rev(&root, "HEAD"), main);

    git(&root, &["checkout", "-q", "-b", "behind", "main~1"]);
    git(&root, &["config", "commit.gpgSign", "true"]);
    repo.merge_into_current(&local("main")).unwrap();
    assert_eq!(rev(&root, "HEAD"), main);
}

#[test]
fn merge_refuses_without_changing_anything() {
    let (_dir, root) = init(false);
    let repo = open(&root);
    let main = rev(&root, "main");

    assert!(matches!(
        repo.merge_into_current(&local("nope")),
        Err(MergeError::NotFound)
    ));

    fs::write(root.join("g"), "untracked").unwrap();
    match repo.merge_into_current(&local("feat")) {
        Err(MergeError::WouldOverwrite { paths }) => {
            assert_eq!(paths, vec![repo.workdir().join("g")]);
        }
        other => panic!("expected WouldOverwrite, got {other:?}"),
    }
    assert_eq!(rev(&root, "HEAD"), main);
    assert_eq!(fs::read_to_string(root.join("g")).unwrap(), "untracked");
    fs::remove_file(root.join("g")).unwrap();

    git(&root, &["checkout", "-q", "--detach"]);
    assert!(matches!(
        repo.merge_into_current(&local("feat")),
        Err(MergeError::Detached)
    ));
    assert_eq!(rev(&root, "HEAD"), main);
}
