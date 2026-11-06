//! Staging, unstaging and discarding more paths than fit in the OS argument limit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{ChangeKind, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
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
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn init() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    (dir, root)
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

#[cfg(unix)]
fn arg_max() -> usize {
    let out = Command::new("getconf").arg("ARG_MAX").output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// The longest command line `CreateProcess` takes, in UTF-16 units.
#[cfg(windows)]
fn arg_max() -> usize {
    32_767
}

#[cfg(unix)]
fn path_max() -> usize {
    let out = Command::new("getconf")
        .args(["PATH_MAX", "/"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// `MAX_PATH`, which Git for Windows keeps to without `core.longpaths`.
#[cfg(windows)]
fn path_max() -> usize {
    260
}

/// Enough file names, in a directory nested to near PATH_MAX (so few files
/// are needed), that their relative paths exceed ARG_MAX. Not nested at all
/// where PATH_MAX is short, as on Windows.
fn many_names() -> Vec<String> {
    let levels = (path_max() / 2).saturating_sub(100) / 251;
    let dir: String = vec!["d".repeat(250); levels]
        .iter()
        .map(|level| format!("{level}/"))
        .collect();
    let each = dir.len() + 12;
    let count = arg_max() / each + 200;
    (0..count).map(|n| format!("{dir}f{n:09}.txt")).collect()
}

fn create(root: &Path, names: &[String], text: &str) -> Vec<PathBuf> {
    fs::create_dir_all(root.join(&names[0]).parent().unwrap()).unwrap();
    names
        .iter()
        .map(|name| {
            let path = root.join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect()
}

fn kinds(repo: &Repo) -> Vec<(Option<ChangeKind>, Option<ChangeKind>)> {
    repo.status_split()
        .unwrap()
        .into_iter()
        .map(|s| (s.staged, s.unstaged))
        .collect()
}

#[test]
fn stage_and_unstage_more_paths_than_arg_max() {
    let (_dir, root) = init();
    let names = many_names();
    let paths = create(&root, &names, "x\n");
    let repo = open(&root);
    for round in 0..2 {
        repo.stage(&paths).unwrap();
        let split = kinds(&repo);
        assert_eq!(split.len(), names.len());
        assert!(split.iter().all(|k| *k == (Some(ChangeKind::Added), None)));
        repo.unstage(&paths).unwrap();
        let split = kinds(&repo);
        assert_eq!(split.len(), names.len());
        assert!(split.iter().all(|k| k.0.is_none() && k.1.is_some()));
        if round == 0 {
            // A first commit, so the second round unstages against a born HEAD.
            fs::write(root.join("first.txt"), "1\n").unwrap();
            git(&root, &["add", "first.txt"]);
            git(&root, &["commit", "-q", "-m", "first"]);
        }
    }
}

#[test]
fn discard_more_paths_than_arg_max() {
    let (_dir, root) = init();
    let names = many_names();
    let mut paths = create(&root, &names, "one\n");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    create(&root, &names, "two\n");
    let extra: Vec<String> = (0..3).map(|n| format!("loose{n}.txt")).collect();
    paths.extend(create(&root, &extra, "loose\n"));
    let repo = open(&root);
    repo.discard(&paths).unwrap();
    assert!(repo.status_split().unwrap().is_empty());
    assert_eq!(fs::read_to_string(&paths[0]).unwrap(), "one\n");
    assert!(extra.iter().all(|name| !root.join(name).exists()));
}

#[test]
fn literal_names_via_stdin() {
    let (_dir, root) = init();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    fs::write(root.join("b"), "b\n").unwrap();
    fs::write(root.join("*.txt"), "star\n").unwrap();
    fs::write(root.join(":(glob)b"), "glob\n").unwrap();
    git(&root, &["add", "a.txt", "b"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let repo = open(&root);
    let status = |repo: &Repo| -> Vec<(String, Option<ChangeKind>)> {
        repo.status_split()
            .unwrap()
            .into_iter()
            .map(|s| {
                let name = s.path.strip_prefix(repo.workdir()).unwrap();
                (name.to_string_lossy().into_owned(), s.staged)
            })
            .collect()
    };
    let odd = [root.join("*.txt"), root.join(":(glob)b")];
    for path in &odd {
        repo.stage(std::slice::from_ref(path)).unwrap();
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let staged: Vec<_> = status(&repo)
            .into_iter()
            .filter(|s| s.1.is_some())
            .collect();
        assert_eq!(staged, vec![(name, Some(ChangeKind::Added))]);
        repo.unstage(std::slice::from_ref(path)).unwrap();
        assert!(status(&repo).iter().all(|s| s.1.is_none()));
    }
    fs::write(root.join("a.txt"), "changed\n").unwrap();
    repo.discard(&odd).unwrap();
    assert!(odd.iter().all(|path| !path.exists()));
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "changed\n");
}

#[test]
fn nul_in_a_path_is_refused() {
    let (_dir, root) = init();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let repo = open(&root);
    let bad = [root.join("a\0.")];
    assert!(repo.stage(&bad).is_err());
    assert!(repo.unstage(&bad).is_err());
    assert!(kinds(&repo).iter().all(|k| k.0.is_none()));
}

#[test]
fn discard_keeps_a_modify_delete_conflict_file() {
    let (_dir, root) = init();
    fs::write(root.join("f.txt"), "base\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "other"]);
    fs::write(root.join("f.txt"), "theirs\n").unwrap();
    git(&root, &["commit", "-qam", "theirs"]);
    git(&root, &["checkout", "-q", "main"]);
    git(&root, &["rm", "-q", "f.txt"]);
    git(&root, &["commit", "-qm", "ours deletes"]);
    let merge = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "commit.gpgsign=false", "merge", "other"])
        .current_dir(&root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let repo = open(&root);
    let _ = repo.discard(&[root.join("f.txt")]);
    assert_eq!(fs::read_to_string(root.join("f.txt")).unwrap(), "theirs\n");
}
