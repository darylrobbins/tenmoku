//! Pasting in the project tree: `fe_project::fs_ops`'s `copy_into`,
//! `paste_name` and `move_into`, which move or copy an entry into another
//! folder under the root, never replacing anything.

use std::{
    fs,
    io::ErrorKind,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use fe_project::{
    fs_ops::{copy_into, exists, move_into, paste_name},
    relative_path,
};
use tempfile::TempDir;

/// A temporary directory and its canonical path (on macOS `/var` is a link to
/// `/private/var`, and the operations want a root reached without one).
fn temp() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    (dir, path)
}

/// Every path under `root`, relative and sorted, folders with a trailing `/`,
/// symlinks with their target, and files with their contents.
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = relative_path(root, &path);
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_symlink() {
                out.push(format!(
                    "{rel} -> {}",
                    fs::read_link(&path).unwrap().display()
                ));
            } else if kind.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &path, out);
            } else {
                out.push(format!(
                    "{rel} = {}",
                    fs::read_to_string(&path).unwrap_or_default()
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[test]
fn copy_into_takes_its_name_then_a_copys() {
    let (_dir, root) = temp();
    write(&root, "a.txt", "a");
    write(&root, "src/keep.rs", "");
    fs::create_dir(root.join("out")).unwrap();

    let copy = copy_into(&root, &root.join("a.txt"), &root.join("out")).unwrap();
    assert_eq!(copy, root.join("out/a.txt"));
    let again = copy_into(&root, &root.join("a.txt"), &root.join("out")).unwrap();
    assert_eq!(again, root.join("out/a copy.txt"));
    let third = copy_into(&root, &root.join("a.txt"), &root.join("out")).unwrap();
    assert_eq!(third, root.join("out/a copy 2.txt"));
    // Into its own folder: beside itself.
    let beside = copy_into(&root, Path::new("a.txt"), Path::new("")).unwrap();
    assert_eq!(beside, root.join("a copy.txt"));
    // A folder, with what's in it.
    let folder = copy_into(&root, &root.join("src"), &root.join("out")).unwrap();
    assert_eq!(folder, root.join("out/src"));
    assert_eq!(
        snapshot(&root),
        [
            "a copy.txt = a",
            "a.txt = a",
            "out/",
            "out/a copy 2.txt = a",
            "out/a copy.txt = a",
            "out/a.txt = a",
            "out/src/",
            "out/src/keep.rs = ",
            "src/",
            "src/keep.rs = ",
        ]
    );
}

#[test]
fn a_folder_is_not_copied_or_moved_into_itself() {
    let (_dir, root) = temp();
    write(&root, "a/b/c.txt", "c");
    let before = snapshot(&root);
    for dir in ["a", "a/b"] {
        let copied = copy_into(&root, &root.join("a"), &root.join(dir));
        assert_eq!(copied.unwrap_err().kind(), ErrorKind::InvalidInput, "{dir}");
        let moved = move_into(&root, &root.join("a"), &root.join(dir), "a");
        assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput, "{dir}");
    }
    assert_eq!(snapshot(&root), before);
    // A folder beside it whose name starts the same is another folder.
    fs::create_dir(root.join("ab")).unwrap();
    assert_eq!(
        move_into(&root, &root.join("a"), &root.join("ab"), "a").unwrap(),
        root.join("ab/a")
    );
}

#[test]
fn move_into_moves_and_never_replaces() {
    let (_dir, root) = temp();
    write(&root, "a.txt", "mine");
    write(&root, "out/a.txt", "theirs");
    write(&root, "src/lib.rs", "lib");

    // Taken: a copy's name.
    let name = paste_name(&root, &root.join("a.txt"), &root.join("out")).unwrap();
    assert_eq!(name, "a copy.txt");
    let moved = move_into(&root, &root.join("a.txt"), &root.join("out"), "a copy.txt").unwrap();
    assert_eq!(moved, root.join("out/a copy.txt"));
    // A name taken by the time it's used fails, replacing nothing.
    let clash = move_into(&root, &root.join("src/lib.rs"), &root.join("out"), "a.txt");
    assert_eq!(clash.unwrap_err().kind(), ErrorKind::AlreadyExists);
    // A folder moves with what's in it.
    let name = paste_name(&root, &root.join("src"), &root.join("out")).unwrap();
    assert_eq!(name, "src");
    move_into(&root, &root.join("src"), &root.join("out"), "src").unwrap();
    // To where it is: nothing to do.
    let same = move_into(&root, &root.join("out/src"), &root.join("out"), "src").unwrap();
    assert_eq!(same, root.join("out/src"));
    assert_eq!(
        snapshot(&root),
        [
            "out/",
            "out/a copy.txt = mine",
            "out/a.txt = theirs",
            "out/src/",
            "out/src/lib.rs = lib",
        ]
    );
}

#[test]
fn paste_name_in_its_own_folder_names_a_copy() {
    let (_dir, root) = temp();
    write(&root, "notes.md", "");
    write(&root, "notes copy.md", "");
    fs::create_dir(root.join("docs")).unwrap();
    assert_eq!(
        paste_name(&root, &root.join("notes.md"), &root).unwrap(),
        "notes copy 2.md"
    );
    assert_eq!(
        paste_name(&root, &root.join("docs"), &root).unwrap(),
        "docs copy"
    );
}

#[test]
fn pastes_through_a_symlink_or_out_of_the_root_are_refused() {
    let (_dir, root) = temp();
    let (_outside, outside) = temp();
    write(&root, "a.txt", "a");
    write(&outside, "secret.txt", "s");
    symlink(&outside, root.join("link")).unwrap();
    let before = (snapshot(&root), snapshot(&outside));

    // Into a symlinked folder.
    let copied = copy_into(&root, &root.join("a.txt"), &root.join("link"));
    assert_eq!(copied.unwrap_err().kind(), ErrorKind::InvalidInput);
    let moved = move_into(&root, &root.join("a.txt"), &root.join("link"), "a.txt");
    assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput);
    // From one.
    let copied = copy_into(&root, &root.join("link/secret.txt"), &root);
    assert_eq!(copied.unwrap_err().kind(), ErrorKind::InvalidInput);
    // Out of the root, or from outside it.
    let moved = move_into(&root, &root.join("a.txt"), &outside, "a.txt");
    assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput);
    let moved = move_into(&root, &root.join("a.txt"), &root.join(".."), "a.txt");
    assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput);
    let copied = copy_into(&root, &outside.join("secret.txt"), &root);
    assert_eq!(copied.unwrap_err().kind(), ErrorKind::InvalidInput);
    // A name that isn't one component.
    let moved = move_into(&root, &root.join("a.txt"), &root, "x/a.txt");
    assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput);
    assert_eq!((snapshot(&root), snapshot(&outside)), before);

    // The link itself copies and moves as a link.
    fs::create_dir(root.join("out")).unwrap();
    copy_into(&root, &root.join("link"), &root.join("out")).unwrap();
    move_into(&root, &root.join("link"), &root.join("out"), "moved").unwrap();
    let target = outside.display().to_string();
    assert_eq!(
        snapshot(&root),
        [
            "a.txt = a".to_owned(),
            "out/".to_owned(),
            format!("out/link -> {target}"),
            format!("out/moved -> {target}"),
        ]
    );
}

/// Whether `dir` is on a volume that ignores case.
fn ignores_case(dir: &Path) -> bool {
    let probe = dir.join("case-probe");
    fs::write(&probe, "").unwrap();
    let ignores = dir.join("CASE-PROBE").exists();
    fs::remove_file(probe).unwrap();
    ignores
}

#[test]
fn another_spelling_of_the_folder_is_still_inside_it() {
    let (_dir, root) = temp();
    if !ignores_case(&root) {
        eprintln!("skipped: {} tells case apart", root.display());
        return;
    }
    write(&root, "A/sub/f.txt", "f");
    let before = snapshot(&root);
    for dir in ["a", "a/sub", "a/SUB"] {
        let copied = copy_into(&root, &root.join("A"), &root.join(dir));
        assert_eq!(copied.unwrap_err().kind(), ErrorKind::InvalidInput, "{dir}");
        let moved = move_into(&root, &root.join("A"), &root.join(dir), "A");
        assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput, "{dir}");
    }
    assert_eq!(snapshot(&root), before);
    // Into the folder it's in, spelled otherwise: nothing to move.
    write(&root, "B/g.txt", "g");
    let same = move_into(&root, &root.join("B/g.txt"), &root.join("b"), "g.txt").unwrap();
    assert_eq!(same, root.join("b/g.txt"));
    assert!(root.join("B/g.txt").is_file());
}

#[test]
fn a_name_that_isnt_utf8_is_moved_and_copied() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let (_dir, root) = temp();
    let name = OsStr::from_bytes(b"caf\xe9.txt");
    if fs::write(root.join(name), "x").is_err() {
        eprintln!("skipped: {} takes only UTF-8 names", root.display());
        return;
    }
    fs::create_dir(root.join("out")).unwrap();
    let pasted = paste_name(&root, &root.join(name), &root.join("out")).unwrap();
    assert_eq!(pasted, name);
    let moved = move_into(&root, &root.join(name), &root.join("out"), &pasted).unwrap();
    assert_eq!(moved, root.join("out").join(name));
    let copy = copy_into(&root, &moved, &root.join("out")).unwrap();
    assert_eq!(
        copy,
        root.join("out")
            .join(OsStr::from_bytes(b"caf\xe9 copy.txt"))
    );
    assert_eq!(fs::read_to_string(&copy).unwrap(), "x");
    // A NUL or a `/` is never a name.
    for bad in [&b"a\xe9/b"[..], b"a\xe9\0b"] {
        let moved = move_into(&root, &moved, &root, OsStr::from_bytes(bad));
        assert_eq!(moved.unwrap_err().kind(), ErrorKind::InvalidInput);
    }
}

#[test]
fn exists_looks_under_the_root_without_following_links() {
    let (_dir, root) = temp();
    let (_outside, outside) = temp();
    write(&root, "a/b.txt", "");
    write(&outside, "c.txt", "");
    symlink(&outside, root.join("link")).unwrap();
    assert!(exists(&root, &root.join("a/b.txt")).unwrap());
    assert!(
        exists(&root, &root.join("link")).unwrap(),
        "the link itself"
    );
    assert!(!exists(&root, &root.join("a/gone.txt")).unwrap());
    assert!(!exists(&root, &root.join("gone/b.txt")).unwrap());
    assert!(
        !exists(&root, &root.join("a/b.txt/c")).unwrap(),
        "a file above"
    );
    let through = exists(&root, &root.join("link/c.txt"));
    assert_eq!(through.unwrap_err().kind(), ErrorKind::InvalidInput);
}
