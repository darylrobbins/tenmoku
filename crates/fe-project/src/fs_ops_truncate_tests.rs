//! [`super::open_truncated`]: it empties a file under the root in place,
//! keeping what it is, and never through a symlink, there or on the way,
//! whenever it appeared.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

use tempfile::TempDir;

use super::*;

/// Links folder `link` to folder `target`: a symlink, or on Windows a
/// junction, which needs no privilege and is a link all the same.
fn dir_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // `mklink` takes neither `\\?\` paths nor `/` separators.
        let plain = |path: &Path| {
            let text = path.to_str().unwrap();
            text.strip_prefix(r"\\?\")
                .unwrap_or(text)
                .replace('/', r"\")
        };
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &plain(link), &plain(target)])
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }
}

/// A project in a temporary folder, with `a/b/f.txt`, and a folder outside
/// it with `b/f.txt` in the same place, which nothing may empty.
fn project() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    let (root, outside) = (base.join("project"), base.join("outside"));
    for top in [root.join("a"), outside.clone()] {
        fs::create_dir_all(top.join("b")).unwrap();
        fs::write(top.join("b/f.txt"), "kept\n").unwrap();
    }
    (dir, root, outside)
}

/// A group of this process's other than its own, if it is in one.
#[cfg(unix)]
fn other_group() -> Option<libc::gid_t> {
    // SAFETY: a zero count asks only how many there are.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    let mut groups = vec![0; usize::try_from(count).ok()?];
    // SAFETY: `groups` has room for `count` ids.
    let count = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
    groups.truncate(usize::try_from(count).ok()?);
    // SAFETY: getegid has no preconditions and can't fail.
    let own = unsafe { libc::getegid() };
    groups.into_iter().find(|&group| group != own)
}

#[test]
fn it_empties_the_file_in_place_keeping_its_mode_and_group() {
    let (_dir, root, _) = project();
    let path = root.join("a/b/f.txt");
    #[cfg(unix)]
    {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        if let Some(group) = other_group() {
            std::os::unix::fs::chown(&path, None, Some(group)).unwrap();
        }
    }
    #[cfg(unix)]
    let before = fs::metadata(&path).unwrap();
    let identity = |path: &Path| listing::identity(&File::open(path).unwrap()).unwrap();
    let before_id = identity(&path);

    // Absolute under the root, or relative to it.
    for path in [path.clone(), PathBuf::from("a/b/f.txt")] {
        fs::write(root.join("a/b/f.txt"), "old\n").unwrap();
        let file = open_truncated(&root, &path).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
    }
    assert_eq!(fs::read(&path).unwrap(), b"");
    assert_eq!(identity(&path), before_id);
    #[cfg(unix)]
    {
        let after = fs::metadata(&path).unwrap();
        assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
        assert_eq!(after.mode() & 0o7777, 0o640);
        assert_eq!(after.gid(), before.gid());
        assert_eq!(after.uid(), before.uid());
    }
}

#[test]
fn it_writes_through_the_handle_it_returns() {
    use std::io::Write as _;

    let (_dir, root, _) = project();
    let mut file = open_truncated(&root, Path::new("a/b/f.txt")).unwrap();
    file.write_all(b"new\n").unwrap();
    drop(file);
    assert_eq!(fs::read(root.join("a/b/f.txt")).unwrap(), b"new\n");
}

#[test]
#[cfg(unix)]
fn it_refuses_a_symlink_in_place_of_the_file() {
    let (_dir, root, outside) = project();
    let link = root.join("a/b/link.txt");
    symlink(outside.join("b/f.txt"), &link).unwrap();
    let err = open_truncated(&root, &link).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    assert_eq!(fs::read(outside.join("b/f.txt")).unwrap(), b"kept\n");
}

#[test]
fn it_refuses_a_symlinked_folder_on_the_way() {
    let (_dir, root, outside) = project();
    fs::rename(root.join("a"), root.join("a.moved")).unwrap();
    dir_link(&outside, &root.join("a"));
    let err = open_truncated(&root, Path::new("a/b/f.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    assert_eq!(fs::read(outside.join("b/f.txt")).unwrap(), b"kept\n");
}

#[test]
// Windows won't rename a folder while a handle holds it (or what is
// in it), as this race must.
#[cfg(unix)]
fn a_folder_swapped_for_a_symlink_once_reached_leads_nowhere_else() {
    let (_dir, root, outside) = project();
    let (a, moved) = (root.join("a"), root.join("a.moved"));
    let link = outside.clone();
    race_tests::at(Point::Truncating, move || {
        fs::rename(&a, &moved).unwrap();
        dir_link(&link, &a);
    });
    let result = open_truncated(&root, Path::new("a/b/f.txt"));
    assert!(race_tests::reached(), "the swap was made");
    // The file in the folder reached is emptied, wherever that folder went;
    // the one the symlink leads to never is.
    result.unwrap();
    assert_eq!(fs::read(root.join("a.moved/b/f.txt")).unwrap(), b"");
    assert_eq!(fs::read(outside.join("b/f.txt")).unwrap(), b"kept\n");
}

#[test]
#[cfg(unix)]
fn a_file_swapped_for_a_symlink_once_its_folder_is_reached_is_refused() {
    let (_dir, root, outside) = project();
    let path = root.join("a/b/f.txt");
    let target = outside.join("b/f.txt");
    let swapped = path.clone();
    race_tests::at(Point::Truncating, move || {
        fs::remove_file(&swapped).unwrap();
        symlink(&target, &swapped).unwrap();
    });
    let err = open_truncated(&root, &path).unwrap_err();
    assert!(race_tests::reached(), "the swap was made");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    assert_eq!(fs::read(outside.join("b/f.txt")).unwrap(), b"kept\n");
}

#[test]
fn it_refuses_what_isnt_a_regular_file() {
    let (_dir, root, _) = project();
    let err = open_truncated(&root, Path::new("a/b")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::IsADirectory, "{err}");

    // FIFOs are Unix's.
    #[cfg(unix)]
    refuses_a_fifo(&root);
}

#[cfg(unix)]
fn refuses_a_fifo(root: &Path) {
    let fifo = root.join("a/fifo");
    let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `name` is NUL-terminated.
    check(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }).unwrap();
    // No reader: the non-blocking open fails rather than waiting for one.
    assert!(open_truncated(root, &fifo).is_err());
    // With one, it opens, and is refused before anything is done to it.
    let _reader = {
        use std::os::unix::fs::OpenOptionsExt as _;
        (fs::OpenOptions::new().read(true))
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap()
    };
    let err = open_truncated(root, &fifo).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
}

#[test]
fn it_never_creates_a_file_or_leaves_the_root() {
    let (_dir, root, outside) = project();
    let err = open_truncated(&root, Path::new("a/b/new.txt")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    assert!(!root.join("a/b/new.txt").exists());
    for path in [Path::new("../outside/b/f.txt"), &outside.join("b/f.txt")] {
        let err = open_truncated(&root, path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    }
    assert_eq!(fs::read(outside.join("b/f.txt")).unwrap(), b"kept\n");
}
