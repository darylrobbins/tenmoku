//! A lane's git directories on Windows are plain paths, never verbatim
//! `\\?\` ones: those are what git and Claude Code write, and what the
//! profile's rules and `denyWrite` are matched against (T1967).

#![cfg(windows)]

use std::path::Path;
use std::process::Command;

use fe_acp::{Lane, LaneDirs};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("running git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn is_verbatim(path: &Path) -> bool {
    path.to_str().unwrap().starts_with(r"\\?\")
}

#[test]
fn lane_dirs_have_no_verbatim_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dunce::canonicalize(dir.path()).unwrap();
    assert!(!is_verbatim(&base));
    let root = base.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("notes.txt"), "notes\n").unwrap();
    git(&root, &["add", "notes.txt"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let worktree = base.join("lane");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lane",
            worktree.to_str().unwrap(),
        ],
    );
    let lane = Lane { root, worktree };

    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane));
    eprintln!("{dirs:?}");
    // Before T1967 the verbatim prefix's `?` refused the lane as a glob.
    assert!(dirs.is_ok());
    let dirs = dirs.unwrap();
    assert!(!is_verbatim(&dirs.common_dir));
    assert!(!is_verbatim(&dirs.worktree_admin_dir));
    assert!(!is_verbatim(&dirs.root_admin_dir));
    assert!(dirs.common_dir == lane.root.join(".git"));
    assert!(dirs.root_admin_dir == dirs.common_dir);
    assert!(dirs.worktree_admin_dir == lane.root.join(".git").join("worktrees").join("lane"));
}
