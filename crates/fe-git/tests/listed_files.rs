//! The files git tracks or would add, against a temp repository.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_git::Repo;

fn git(dir: &Path, args: &[&str]) {
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
}

#[test]
fn listed_files_returns_tracked_and_untracked_unignored_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join(".gitignore"), "*.log\nbuild/\n").unwrap();
    fs::write(root.join("src/tracked.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("gone.txt"), "deleted later\n").unwrap();
    git(root, &["add", "."]);
    // Untracked, but not ignored.
    fs::write(root.join("new.txt"), "new\n").unwrap();
    // Ignored, by name and by folder.
    fs::write(root.join("debug.log"), "log\n").unwrap();
    fs::create_dir(root.join("build")).unwrap();
    fs::write(root.join("build/out.o"), "o\n").unwrap();
    // Tracked, but no longer on disk.
    fs::remove_file(root.join("gone.txt")).unwrap();
    // An untracked nested repository is a folder, not a file.
    fs::create_dir(root.join("nested")).unwrap();
    git(&root.join("nested"), &["init", "-q"]);

    let repo = Repo::discover(&root.join("src")).unwrap().expect("a repo");
    let workdir = repo.workdir().to_owned();
    let listed = repo.listed_files().unwrap();
    let names: Vec<String> = (listed.iter())
        .map(|path| {
            assert!(path.is_absolute(), "{path:?}");
            let rest = path.strip_prefix(&workdir).unwrap();
            rest.to_string_lossy().replace('\\', "/")
        })
        .collect();
    assert_eq!(names, [".gitignore", "new.txt", "src/tracked.rs"]);

    // The index is written once something was added.
    assert!(repo.index_mtime().is_some());
}
