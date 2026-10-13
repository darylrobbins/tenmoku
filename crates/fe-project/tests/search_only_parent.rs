//! A project beneath a folder that can be searched but not listed is
//! reached through the public API.

#![cfg(unix)]

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use fe_project::{FileTree, FinderIndex, TreeOptions, open_file_under};

/// `locked/` (mode 0o111) above `project/` holding `a.rs` and `src/b.rs`.
/// None when permissions don't bind (root).
fn locked_project() -> Option<(tempfile::TempDir, PathBuf)> {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let project = root.join("locked/project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("a.rs"), "fn a() {}\n").unwrap();
    fs::write(project.join("src/b.rs"), "fn b() {}\n").unwrap();
    // SAFETY: `geteuid` has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return None;
    }
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o111)).unwrap();
    Some((tmp, project))
}

fn unlock(tmp: &tempfile::TempDir) {
    let locked = fs::canonicalize(tmp.path()).unwrap().join("locked");
    fs::set_permissions(locked, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn tree_lists_a_project_beneath_a_search_only_folder() {
    let Some((tmp, project)) = locked_project() else {
        return;
    };
    let names = FileTree::open(&project, TreeOptions::default()).map(|tree| {
        let kids = tree.children(tree.root()).expect("the root is listed");
        kids.iter()
            .map(|&id| tree.row(id).unwrap().name.to_os_string())
            .collect::<Vec<_>>()
    });
    unlock(&tmp);
    let names = names.expect("the project opens");
    assert!(names.iter().any(|n| n == "a.rs"), "{names:?}");
    assert!(names.iter().any(|n| n == "src"), "{names:?}");
}

#[test]
fn finder_walks_a_project_beneath_a_search_only_folder() {
    let Some((tmp, project)) = locked_project() else {
        return;
    };
    let mut paths = Vec::new();
    let done = FinderIndex::walk(&project, Arc::new(AtomicBool::new(false)), |batch| {
        paths.extend(batch);
    });
    unlock(&tmp);
    assert!(done);
    assert!(paths.contains(&PathBuf::from("a.rs")), "{paths:?}");
    assert!(paths.contains(&PathBuf::from("src/b.rs")), "{paths:?}");
}

#[test]
fn open_file_under_reads_beneath_a_search_only_folder() {
    let Some((tmp, project)) = locked_project() else {
        return;
    };
    let file = open_file_under(&project, Path::new("src/b.rs"));
    unlock(&tmp);
    let mut text = String::new();
    file.expect("the file opens")
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "fn b() {}\n");
}
