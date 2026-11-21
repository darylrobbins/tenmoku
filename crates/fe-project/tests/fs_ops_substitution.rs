//! Another process of the same user swapping an entry in at a boundary of an
//! fs_ops operation, run at that boundary through `fs_ops::testing::at`
//! (never by timing): what it swaps in for a folder or symlink just made is
//! never recorded as made, and a restore that publishes anything but the
//! entry it checked fails and moves nothing back. Trashes go to the test
//! trash of the process's own, never the user's.
#![cfg(unix)]

use std::{
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use fe_project::fs_ops::{
    self,
    testing::{self, Boundary},
};

/// A temporary project, by its canonical path.
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("fe-project-substitution-")
        .tempdir()
        .unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    (dir, root)
}

/// What the entry at `path` is, as the workbench looks it up, birth time
/// and all.
fn id(path: &Path) -> fs_ops::EntryId {
    fs_ops::entry_identity(path).unwrap()
}

/// The names in folder `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

/// Moves what is at `path` to `aside`, and puts a folder of another's in
/// its place, holding `theirs.txt`, its permissions 0o755.
fn swap_in_folder(path: PathBuf, aside: PathBuf) -> impl FnOnce() {
    move || {
        fs::rename(&path, &aside).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("theirs.txt"), "theirs\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn entries_made_without_a_swap_are_recorded_as_what_they_are() {
    let (_dir, root) = project();
    let created = fs_ops::create_recorded(&root, &root, "a/b/", true).unwrap();
    assert_eq!(created.first.path, root.join("a"));
    assert_eq!(created.first.id, id(&root.join("a")));

    symlink("target.txt", root.join("link")).unwrap();
    let copy = fs_ops::duplicate_recorded(&root, &root.join("link")).unwrap();
    assert_eq!(copy.path, root.join("link copy"));
    assert_eq!(copy.id, id(&copy.path));
    assert_eq!(fs::read_link(&copy.path).unwrap(), Path::new("target.txt"));
}

#[test]
fn a_folder_swapped_in_for_a_created_folder_is_not_recorded() {
    let (_dir, root) = project();
    let made = root.join("new");
    testing::at(
        Boundary::Made,
        swap_in_folder(made.clone(), root.join("ours")),
    );

    let err = fs_ops::create_recorded(&root, &root, "new", true).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    assert_eq!(names(&made), ["theirs.txt"]);
    assert_eq!(mode(&made), 0o755);
}

#[test]
fn a_folder_swapped_in_for_one_above_a_created_file_is_not_recorded() {
    let (_dir, root) = project();
    let made = root.join("a");
    testing::at(
        Boundary::Made,
        swap_in_folder(made.clone(), root.join("ours")),
    );

    let err = fs_ops::create_recorded(&root, &root, "a/b.txt", false).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // Left as it was: nothing made in it, nothing of it removed.
    assert_eq!(names(&made), ["theirs.txt"]);
    assert_eq!(mode(&made), 0o755);
    assert_eq!(names(&root), ["a", "ours"]);
}

#[test]
fn a_folder_swapped_in_for_a_copied_folder_is_not_recorded() {
    let (_dir, root) = project();
    let source = root.join("src");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.rs"), "fn main() {}\n").unwrap();
    let copy = root.join("src copy");
    testing::at(
        Boundary::Made,
        swap_in_folder(copy.clone(), root.join("ours")),
    );

    let err = fs_ops::duplicate_recorded(&root, &source).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // Its permissions as they were, nothing copied into it.
    assert_eq!(names(&copy), ["theirs.txt"]);
    assert_eq!(mode(&copy), 0o755);
    assert_eq!(names(&source), ["main.rs"]);
}

#[test]
fn a_link_swapped_in_for_a_copied_link_is_not_recorded() {
    let (_dir, root) = project();
    symlink("target.txt", root.join("link")).unwrap();
    let copy = root.join("link copy");
    testing::at(Boundary::Made, {
        let (copy, aside) = (copy.clone(), root.join("ours"));
        move || {
            fs::rename(&copy, &aside).unwrap();
            symlink("elsewhere.txt", &copy).unwrap();
        }
    });

    let err = fs_ops::duplicate_recorded(&root, &root.join("link")).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    assert_eq!(fs::read_link(&copy).unwrap(), Path::new("elsewhere.txt"));
}

#[test]
fn another_name_of_a_link_swapped_in_for_a_copied_link_is_not_recorded() {
    let (_dir, root) = project();
    symlink("target.txt", root.join("link")).unwrap();
    let copy = root.join("link copy");
    // A link to the same place, but one with another name: not new.
    let theirs = root.join("theirs");
    symlink("target.txt", &theirs).unwrap();
    testing::at(Boundary::Made, {
        let (copy, aside, theirs) = (copy.clone(), root.join("ours"), theirs.clone());
        move || {
            fs::rename(&copy, &aside).unwrap();
            fs::hard_link(&theirs, &copy).unwrap();
        }
    });

    let err = fs_ops::duplicate_recorded(&root, &root.join("link")).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    assert_eq!(id(&copy), id(&theirs));
}

#[test]
fn a_file_swapped_in_for_a_link_in_a_copied_folder_is_kept() {
    let (_dir, root) = project();
    let source = root.join("src");
    fs::create_dir(&source).unwrap();
    symlink("main.rs", source.join("link")).unwrap();
    let swapped = root.join("src copy/link");
    // The copy's folder is made first, then the link in it.
    testing::at(Boundary::Made, {
        let (swapped, aside) = (swapped.clone(), root.join("ours"));
        move || {
            testing::at(Boundary::Made, move || {
                fs::rename(&swapped, &aside).unwrap();
                fs::write(&swapped, "theirs\n").unwrap();
            });
        }
    });

    let err = fs_ops::duplicate_recorded(&root, &source).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // Never taken for the copy's link, so never removed with the copy.
    assert_eq!(fs::read_to_string(&swapped).unwrap(), "theirs\n");
}

/// Trashes `path`, a file holding "trashed", into the test trash.
fn trash_file(root: &Path, path: &Path) -> fs_ops::Trashed {
    fs::write(path, "trashed\n").unwrap();
    let trashed = fs_ops::trash_recorded(root, &[path])
        .unwrap()
        .pop()
        .unwrap();
    assert!(
        trashed
            .location()
            .unwrap()
            .starts_with(testing::trash_dir())
    );
    trashed
}

/// The folder `restore` stages entries in, beside `at` in the trash.
fn staging(at: &Path) -> PathBuf {
    let found = fs::read_dir(at.parent().unwrap())
        .unwrap()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".fastedit-restore-")
        });
    found.expect("a staging folder").path()
}

#[test]
fn a_folder_swapped_in_for_a_restores_staging_folder_is_not_used() {
    let (_dir, root) = project();
    let file = root.join("a.txt");
    let mut trashed = trash_file(&root, &file);
    let at = trashed.location().unwrap().to_owned();
    testing::at(Boundary::Made, {
        let at = at.clone();
        move || {
            let staging = staging(&at);
            let aside = at.with_file_name("ours");
            swap_in_folder(staging, aside)();
        }
    });

    let err = fs_ops::restore(&root, &mut trashed).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // Nothing moved: the entry is where the record says, and its folder's
    // stays as it was.
    assert!(!file.exists());
    assert_eq!(trashed.location(), Some(at.as_path()));
    assert!(trashed.in_trash().unwrap());
    assert_eq!(names(&staging(&at)), ["theirs.txt"]);

    assert_eq!(fs_ops::restore(&root, &mut trashed).unwrap(), file);
    assert_eq!(fs::read_to_string(&file).unwrap(), "trashed\n");
}

#[test]
fn a_restore_that_publishes_an_entry_swapped_in_for_the_staged_one_fails() {
    let (_dir, root) = project();
    let file = root.join("a.txt");
    let mut trashed = trash_file(&root, &file);
    let at = trashed.location().unwrap().to_owned();
    let aside = root.join("aside.txt");
    testing::at(Boundary::Staged, {
        let (at, aside) = (at.clone(), aside.clone());
        move || {
            let staged = staging(&at).join("a.txt");
            fs::rename(&staged, &aside).unwrap();
            fs::write(&staged, "another\n").unwrap();
        }
    });

    let err = fs_ops::restore(&root, &mut trashed).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    // What was published stays where it landed, never moved back.
    assert_eq!(fs::read_to_string(&file).unwrap(), "another\n");
    assert!(!at.exists());
    // The record follows nothing: Undo drops it.
    assert_eq!(trashed.location(), None);
    assert!(!trashed.in_trash().unwrap());
    assert_eq!(fs::read_to_string(&aside).unwrap(), "trashed\n");
}

#[test]
fn a_restore_whose_published_entry_is_replaced_fails() {
    let (_dir, root) = project();
    let file = root.join("a.txt");
    let mut trashed = trash_file(&root, &file);
    let at = trashed.location().unwrap().to_owned();
    let aside = root.join("aside.txt");
    testing::at(Boundary::Published, {
        let (file, aside) = (file.clone(), aside.clone());
        move || {
            fs::rename(&file, &aside).unwrap();
            fs::write(&file, "another\n").unwrap();
        }
    });

    let err = fs_ops::restore(&root, &mut trashed).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "another\n");
    assert!(!at.exists(), "nothing moved back into the trash");
    assert_eq!(trashed.location(), None);
    assert!(!trashed.in_trash().unwrap());
    assert_eq!(fs::read_to_string(&aside).unwrap(), "trashed\n");
}

/// What another process puts in place of a folder just made.
#[derive(Clone, Copy, Debug)]
enum Occupant {
    /// A file holding "theirs".
    File,
    /// A symlink to `theirs.txt`.
    Link,
    /// A folder holding `theirs.txt` that its owner can search but not
    /// read (0o300), so it can't be told empty.
    Closed,
}

const OCCUPANTS: [Occupant; 2] = [Occupant::File, Occupant::Link];

/// Moves what is at `path` to `aside` and puts `occupant` in its place.
fn swap_in(path: PathBuf, aside: PathBuf, occupant: Occupant) -> impl FnOnce() {
    move || {
        fs::rename(&path, &aside).unwrap();
        match occupant {
            Occupant::File => fs::write(&path, "theirs\n").unwrap(),
            Occupant::Link => symlink("theirs.txt", &path).unwrap(),
            Occupant::Closed => {
                fs::create_dir(&path).unwrap();
                fs::write(path.join("theirs.txt"), "theirs\n").unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o300)).unwrap();
            }
        }
    }
}

/// Checks `occupant` is still at `path` as it was put there: never removed,
/// changed or filled. A closed folder is opened to its owner again after,
/// so the project can be removed.
fn assert_kept(path: &Path, occupant: Occupant) {
    match occupant {
        Occupant::File => assert_eq!(fs::read_to_string(path).unwrap(), "theirs\n"),
        Occupant::Link => assert_eq!(fs::read_link(path).unwrap(), Path::new("theirs.txt")),
        Occupant::Closed => {
            assert_eq!(mode(path), 0o300);
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(names(path), ["theirs.txt"]);
        }
    }
}

/// Whether this process reads any folder, as root does.
fn is_root() -> bool {
    // SAFETY: always safe to call.
    let root = unsafe { libc::geteuid() } == 0;
    if root {
        eprintln!("skipped: root reads any folder");
    }
    root
}

#[test]
fn a_file_or_link_swapped_in_for_a_folder_above_a_created_file_is_kept() {
    for occupant in OCCUPANTS {
        let (_dir, root) = project();
        let made = root.join("a");
        testing::at(
            Boundary::Made,
            swap_in(made.clone(), root.join("ours"), occupant),
        );

        let err = fs_ops::create_recorded(&root, &root, "a/b.txt", false).unwrap_err();

        assert!(testing::reached());
        assert_eq!(
            err.kind(),
            io::ErrorKind::AlreadyExists,
            "{occupant:?}: {err}"
        );
        assert_kept(&made, occupant);
    }
}

#[test]
fn a_file_or_link_swapped_in_for_a_copied_folder_is_kept() {
    for occupant in OCCUPANTS {
        let (_dir, root) = project();
        let source = root.join("src");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("main.rs"), "fn main() {}\n").unwrap();
        let copy = root.join("src copy");
        testing::at(
            Boundary::Made,
            swap_in(copy.clone(), root.join("ours"), occupant),
        );

        let err = fs_ops::duplicate_recorded(&root, &source).unwrap_err();

        assert!(testing::reached());
        assert_eq!(
            err.kind(),
            io::ErrorKind::AlreadyExists,
            "{occupant:?}: {err}"
        );
        assert_kept(&copy, occupant);
    }
}

#[test]
fn a_file_or_link_swapped_in_for_a_folder_in_a_copy_is_kept() {
    for occupant in OCCUPANTS {
        let (_dir, root) = project();
        let source = root.join("src");
        fs::create_dir_all(source.join("sub")).unwrap();
        fs::write(source.join("a.txt"), "a\n").unwrap();
        fs::write(source.join("sub/inner.txt"), "inner\n").unwrap();
        let swapped = root.join("src copy/sub");
        // The copy's own folder is made first, then the one in it.
        testing::at(Boundary::Made, {
            let (swapped, aside) = (swapped.clone(), root.join("ours"));
            move || testing::at(Boundary::Made, swap_in(swapped, aside, occupant))
        });

        let err = fs_ops::duplicate_recorded(&root, &source).unwrap_err();

        assert!(testing::reached());
        assert_eq!(
            err.kind(),
            io::ErrorKind::AlreadyExists,
            "{occupant:?}: {err}"
        );
        // Never taken for the copy's, so never removed with the copy.
        assert_kept(&swapped, occupant);
    }
}

#[test]
fn a_full_folder_its_owner_cannot_read_is_never_taken_for_a_created_one() {
    if is_root() {
        return;
    }
    let occupant = Occupant::Closed;
    for name in ["new/", "new/b.txt"] {
        let (_dir, root) = project();
        let made = root.join("new");
        testing::at(
            Boundary::Made,
            swap_in(made.clone(), root.join("ours"), occupant),
        );

        let err = fs_ops::create_recorded(&root, &root, name, false).unwrap_err();

        // It can't be told empty: neither recorded, used nor removed.
        assert!(testing::reached());
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{name}: {err}");
        assert_kept(&made, occupant);
    }
}

#[test]
fn a_full_folder_its_owner_cannot_read_is_never_a_restores_staging_folder() {
    if is_root() {
        return;
    }
    let (_dir, root) = project();
    let file = root.join("a.txt");
    let mut trashed = trash_file(&root, &file);
    let at = trashed.location().unwrap().to_owned();
    let swapped = std::rc::Rc::new(std::cell::RefCell::new(None));
    testing::at(Boundary::Made, {
        let (at, swapped) = (at.clone(), swapped.clone());
        move || {
            let staging = staging(&at);
            *swapped.borrow_mut() = Some(staging.clone());
            swap_in(staging, at.with_file_name("ours"), Occupant::Closed)();
        }
    });

    let err = fs_ops::restore(&root, &mut trashed).unwrap_err();

    assert!(testing::reached());
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
    assert_kept(swapped.borrow().as_deref().unwrap(), Occupant::Closed);
    assert!(!file.exists());
    assert_eq!(trashed.location(), Some(at.as_path()));
    assert!(trashed.in_trash().unwrap());

    assert_eq!(fs_ops::restore(&root, &mut trashed).unwrap(), file);
    assert_eq!(fs::read_to_string(&file).unwrap(), "trashed\n");
}

#[test]
fn a_created_folder_that_cannot_be_confirmed_is_removed() {
    // Under a folder that was there.
    let (_dir, root) = project();
    testing::fail_at(Boundary::Confirming);
    let err = fs_ops::create_recorded(&root, &root, "new/", false).unwrap_err();
    assert!(testing::reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert!(names(&root).is_empty());

    // Under folders the create made, which go with it.
    testing::at(Boundary::Confirming, || {
        testing::fail_at(Boundary::Confirming);
    });
    let err = fs_ops::create_recorded(&root, &root, "a/b/", false).unwrap_err();
    assert!(testing::reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert!(names(&root).is_empty());
}
