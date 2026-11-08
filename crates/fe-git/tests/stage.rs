//! Staging, unstaging, discarding and setting index text, against temp repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{ChangeKind, GitError, Repo};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git_output(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
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
        .expect("run git")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_output(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn init() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    (dir, root)
}

fn write(root: &Path, rela: &str, contents: &[u8]) {
    let path = root.join(rela);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

const ISOLATED_ENV: &str = "FE_GIT_STAGE_ISOLATED";

/// Reruns the calling test in a child whose whole environment shuts out the
/// developer's git config, so `Repo`'s own git calls are isolated too. True in
/// the child, which runs the test body.
fn isolated(name: &str) -> bool {
    if std::env::var_os(ISOLATED_ENV).is_some() {
        return true;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(ISOLATED_ENV, "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

type Split = Vec<(String, Option<ChangeKind>, Option<ChangeKind>)>;

fn split(repo: &Repo) -> Split {
    repo.status_split()
        .unwrap()
        .into_iter()
        .map(|s| {
            let name = s.path.strip_prefix(repo.workdir()).unwrap();
            (name.to_string_lossy().into_owned(), s.staged, s.unstaged)
        })
        .collect()
}

fn s(
    name: &str,
    staged: Option<ChangeKind>,
    unstaged: Option<ChangeKind>,
) -> (String, Option<ChangeKind>, Option<ChangeKind>) {
    (name.to_owned(), staged, unstaged)
}

#[test]
fn files() {
    if !isolated("files") {
        return;
    }
    use ChangeKind::*;
    let (_dir, root) = init();
    write(&root, "mod.txt", b"one\n");
    write(&root, "del.txt", b"gone\n");
    write(&root, "keep.txt", b"keep\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    write(&root, "mod.txt", b"two\n");
    fs::remove_file(root.join("del.txt")).unwrap();
    write(&root, "new/untracked.txt", b"loose\n");
    let repo = open(&root);
    let paths: Vec<PathBuf> = ["mod.txt", "del.txt", "new/untracked.txt"]
        .iter()
        .map(|name| root.join(name))
        .collect();

    repo.stage(&paths).unwrap();
    assert_eq!(
        split(&repo),
        vec![
            s("del.txt", Some(Deleted), None),
            s("mod.txt", Some(Modified), None),
            s("new/untracked.txt", Some(Added), None),
        ]
    );

    repo.unstage(&paths).unwrap();
    assert_eq!(
        split(&repo),
        vec![
            s("del.txt", None, Some(Deleted)),
            s("mod.txt", None, Some(Modified)),
            s("new/untracked.txt", None, Some(Untracked)),
        ]
    );

    repo.discard(&paths).unwrap();
    assert_eq!(split(&repo), vec![]);
    assert_eq!(fs::read_to_string(root.join("mod.txt")).unwrap(), "one\n");
    assert_eq!(fs::read_to_string(root.join("del.txt")).unwrap(), "gone\n");
    assert!(!root.join("new/untracked.txt").exists());
    assert!(root.join("keep.txt").exists());

    // Discard goes back to the index, not HEAD.
    write(&root, "mod.txt", b"staged\n");
    repo.stage(&[root.join("mod.txt")]).unwrap();
    write(&root, "mod.txt", b"worktree\n");
    repo.discard(&[root.join("mod.txt")]).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("mod.txt")).unwrap(),
        "staged\n"
    );
    assert_eq!(split(&repo), vec![s("mod.txt", Some(Modified), None)]);

    // File names that look like pathspecs are taken literally. (Windows
    // names can't hold `*`; `[x].txt` as a glob still matches `x.txt`.)
    let odd = if cfg!(windows) { "[x].txt" } else { "[x]*.txt" };
    write(&root, odd, b"odd\n");
    write(&root, "x.txt", b"other\n");
    repo.stage(&[root.join(odd)]).unwrap();
    assert_eq!(
        split(&repo),
        vec![
            s(odd, Some(Added), None),
            s("mod.txt", Some(Modified), None),
            s("x.txt", None, Some(Untracked)),
        ]
    );
}

#[test]
fn unstage_with_unborn_head() {
    if !isolated("unstage_with_unborn_head") {
        return;
    }
    use ChangeKind::*;
    let (_dir, root) = init();
    write(&root, "a.txt", b"a\n");
    write(&root, "dir/b.txt", b"b\n");
    let repo = open(&root);
    let paths = vec![root.join("a.txt"), root.join("dir/b.txt")];
    repo.stage(&paths).unwrap();
    assert_eq!(
        split(&repo),
        vec![
            s("a.txt", Some(Added), None),
            s("dir/b.txt", Some(Added), None)
        ]
    );
    // Staged text differing from the working tree is still unstaged, and the file kept.
    repo.set_index_text(&root.join("a.txt"), "partial\n")
        .unwrap();
    write(&root, "a.txt", b"edited\n");
    repo.unstage(&paths).unwrap();
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "edited\n");
    assert_eq!(
        split(&repo),
        vec![
            s("a.txt", None, Some(Untracked)),
            s("dir/b.txt", None, Some(Untracked)),
        ]
    );
    assert!(root.join("a.txt").exists());
}

#[test]
fn set_index_text() {
    if !isolated("set_index_text") {
        return;
    }
    use ChangeKind::*;
    let (_dir, root) = init();
    write(&root, "a.txt", b"one\ntwo\n");
    write(&root, "run.sh", b"#!/bin/sh\n");
    git(&root, &["add", "."]);
    git(&root, &["update-index", "--chmod=+x", "run.sh"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    write(&root, "a.txt", b"one\nthree\n");
    let repo = open(&root);

    repo.set_index_text(&root.join("a.txt"), "one\nthree\nfour\n")
        .unwrap();
    assert_eq!(
        repo.index_text(&root.join("a.txt")).unwrap().as_deref(),
        Some("one\nthree\nfour\n")
    );
    assert_eq!(
        fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\nthree\n"
    );

    repo.set_index_text(&root.join("run.sh"), "#!/bin/sh\necho hi\n")
        .unwrap();
    assert_eq!(
        repo.index_text(&root.join("run.sh")).unwrap().as_deref(),
        Some("#!/bin/sh\necho hi\n")
    );
    let listing = git(&root, &["ls-files", "-s", "run.sh"]);
    assert!(listing.starts_with("100755 "), "{listing}");

    // A new entry, regular and executable on disk.
    write(&root, "new.txt", b"fresh\n");
    repo.set_index_text(&root.join("new.txt"), "fre\n").unwrap();
    assert_eq!(
        repo.index_text(&root.join("new.txt")).unwrap().as_deref(),
        Some("fre\n")
    );
    assert!(git(&root, &["ls-files", "-s", "new.txt"]).starts_with("100644 "));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        write(&root, "tool.sh", b"x\n");
        fs::set_permissions(root.join("tool.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        repo.set_index_text(&root.join("tool.sh"), "x\n").unwrap();
        assert!(git(&root, &["ls-files", "-s", "tool.sh"]).starts_with("100755 "));
    }
    assert!(split(&repo).contains(&s("new.txt", Some(Added), Some(Modified))));

    // autocrlf stores LF for CRLF text.
    git(&root, &["config", "core.autocrlf", "true"]);
    repo.set_index_text(&root.join("a.txt"), "one\r\ntwo\r\n")
        .unwrap();
    assert_eq!(
        repo.index_text(&root.join("a.txt")).unwrap().as_deref(),
        Some("one\ntwo\n")
    );
}

const CHILD_ENV: &str = "FE_GIT_STAGE_NO_GIT";

/// Re-run by `errors` with an empty PATH; a no-op otherwise.
#[test]
fn git_missing_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let (_dir, root) = init_without_git_cli();
    let repo = open(&root);
    let err = repo.stage(&[root.join("a.txt")]).unwrap_err();
    assert!(matches!(err, GitError::GitMissing), "{err:?}");
}

fn init_without_git_cli() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // A bare `.git` skeleton gix can discover, built before PATH was emptied.
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::create_dir_all(root.join(".git/refs")).unwrap();
    fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        root.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n",
    )
    .unwrap();
    write(&root, "a.txt", b"a\n");
    (dir, root)
}

#[test]
fn errors() {
    if !isolated("errors") {
        return;
    }
    let (_dir, root) = init();
    write(&root, "a.txt", b"a\n");
    let repo = open(&root);
    let outside = tempfile::tempdir().unwrap();
    let elsewhere = outside.path().join("f.txt");
    for result in [
        repo.stage(std::slice::from_ref(&elsewhere)),
        repo.unstage(std::slice::from_ref(&elsewhere)),
        repo.discard(std::slice::from_ref(&elsewhere)),
        repo.set_index_text(&elsewhere, "x"),
        repo.stage(&[root.join("../escape.txt")]),
        repo.stage(std::slice::from_ref(&root)),
    ] {
        assert!(
            matches!(result, Err(GitError::OutsideWorktree(_))),
            "{result:?}"
        );
    }
    assert!(repo.stage(&[]).is_ok());

    // Repository metadata is off limits.
    for target in [
        root.join(".git"),
        root.join(".git/config"),
        root.join(".git/HEAD"),
    ] {
        let result = repo.discard(std::slice::from_ref(&target));
        assert!(
            matches!(result, Err(GitError::OutsideWorktree(_))),
            "{result:?}"
        );
        assert!(target.exists());
    }

    // A directory isn't discarded, tracked files in it included.
    write(&root, "dir/tracked.txt", b"t\n");
    git(&root, &["add", "dir/tracked.txt"]);
    git(&root, &["commit", "-q", "-m", "c"]);
    write(&root, "dir/loose.txt", b"l\n");
    assert!(repo.discard(&[root.join("dir")]).is_err());
    assert!(root.join("dir/tracked.txt").exists() && root.join("dir/loose.txt").exists());

    // A symlink out of the tree isn't followed to delete.
    #[cfg(unix)]
    {
        let victim = outside.path().join("victim.txt");
        fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        let result = repo.discard(&[root.join("link/victim.txt")]);
        assert!(
            matches!(result, Err(GitError::OutsideWorktree(_))),
            "{result:?}"
        );
        assert!(victim.exists());

        // Nor through a symlink to .git.
        std::os::unix::fs::symlink(root.join(".git"), root.join("meta")).unwrap();
        let result = repo.discard(&[root.join("meta/config")]);
        assert!(
            matches!(result, Err(GitError::OutsideWorktree(_))),
            "{result:?}"
        );
        assert!(root.join(".git/config").exists());
    }

    // git's own failure carries its stderr.
    fs::write(root.join(".git/index.lock"), b"").unwrap();
    let err = repo.stage(&[root.join("a.txt")]).unwrap_err();
    let GitError::Command { args, code, stderr } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(args[0], "add");
    assert_eq!(*code, Some(128));
    assert!(stderr.contains("index.lock"), "{stderr}");
    fs::remove_file(root.join(".git/index.lock")).unwrap();

    // No git on PATH.
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "git_missing_child", "--nocapture"])
        .env(CHILD_ENV, "1")
        .env("PATH", "")
        .status()
        .unwrap();
    assert!(status.success());
}
