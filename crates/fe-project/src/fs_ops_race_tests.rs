//! What another process racing [`super`]'s operations can do: replace the
//! folders of a copy before they get their permissions, put entries in a
//! copy or in place of what an operation made before it is undone, and, on
//! Linux, leave `statx` unable to tell which mount an entry is on. Each race
//! is run at a [`Point`] the operation reaches, never by timing.

#[cfg(target_os = "linux")]
use std::cell::Cell;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{cell::RefCell, fs};

use tempfile::TempDir;

use super::*;

/// What a test runs at a point, which may fail in place of the step that
/// follows.
type Hook = Box<dyn FnOnce() -> io::Result<()>>;

thread_local! {
    /// What [`reach`] runs, once, when this thread reaches its point.
    static HOOK: RefCell<Option<(Point, Hook)>> = const { RefCell::new(None) };
}

/// See [`super::reach`].
pub(super) fn reach(point: Point) -> io::Result<()> {
    let hook = HOOK.with_borrow_mut(|slot| slot.take_if(|(at, _)| *at == point));
    match hook {
        Some((_, hook)) => hook(),
        None => Ok(()),
    }
}

/// Sets `hook` to run the next time this thread reaches `point`.
pub(super) fn at(point: Point, hook: impl FnOnce() + 'static) {
    HOOK.set(Some((
        point,
        Box::new(|| {
            hook();
            Ok(())
        }),
    )));
}

/// Makes the step after `point` fail, the next time this thread reaches it,
/// as an open that ran out of descriptors.
fn fail_at(point: Point) {
    at_then_fail(point, || {});
}

/// Runs `hook` the next time this thread reaches `point`, then fails the
/// step after it as [`fail_at`] does.
fn at_then_fail(point: Point, hook: impl FnOnce() + 'static) {
    HOOK.set(Some((
        point,
        Box::new(|| {
            hook();
            Err(io::Error::from_raw_os_error(libc::EMFILE))
        }),
    )));
}

/// Whether the hook last set has run.
pub(super) fn reached() -> bool {
    HOOK.with_borrow(Option::is_none)
}

/// How telling an entry's mount fails, standing in for a kernel or a file
/// system that can't.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    None,
    /// `statx` fails.
    StatxFails,
    /// `statx` leaves `STATX_MNT_ID` out of its mask.
    NoMountId,
    /// `statx` fails and there is no `/proc/self/fdinfo`.
    Unidentified,
}

#[cfg(target_os = "linux")]
thread_local! {
    /// How telling a mount fails on this thread.
    pub(super) static FAULT: Cell<Fault> = const { Cell::new(Fault::None) };
}

fn temp() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dunce::canonicalize(dir.path()).unwrap();
    (dir, path)
}

/// Gives folder `path` permissions `mode`: nothing on Windows, whose folders
/// have none.
fn set_mode(path: &Path, mode: u32) {
    #[cfg(windows)]
    let _ = (path, mode);
    #[cfg(unix)]
    fs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

/// The names in folder `path`, sorted.
fn names(path: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// A folder `src` to copy, with a file and a folder `sub` with a file in it,
/// all open to anyone to read.
fn source(root: &Path) {
    fs::create_dir_all(root.join("src/sub")).unwrap();
    fs::write(root.join("src/a.txt"), "a").unwrap();
    fs::write(root.join("src/sub/inner.txt"), "inner").unwrap();
    set_mode(&root.join("src/sub"), 0o755);
    set_mode(&root.join("src"), 0o755);
}

/// Replaces folder `path` with a private one holding a file of its own,
/// moving it to `moved`.
#[cfg(unix)]
fn replace_with_private(path: &Path, moved: &Path) {
    fs::rename(path, moved).unwrap();
    fs::create_dir(path).unwrap();
    fs::write(path.join("private.txt"), "theirs").unwrap();
    set_mode(path, 0o700);
}

// C1 (F11): a copy's folders replaced before they get their permissions.

#[test]
#[cfg(unix)]
fn a_copy_whose_top_folder_is_replaced_leaves_the_replacement_as_it_is() {
    let (_dir, root) = temp();
    source(&root);
    let copy = root.join("src copy");
    let moved = root.join("moved");
    at(Point::Copied, {
        let (copy, moved) = (copy.clone(), moved.clone());
        move || replace_with_private(&copy, &moved)
    });

    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // Not given the source's permissions, nor removed as the failed copy.
    assert_eq!(mode(&copy), 0o700);
    assert_eq!(names(&copy), ["private.txt"]);
    assert_eq!(
        fs::read_to_string(copy.join("private.txt")).unwrap(),
        "theirs"
    );
    // The copy made, wherever it was moved, is left there.
    assert_eq!(names(&moved), ["a.txt", "sub"]);
}

#[test]
#[cfg(unix)]
fn a_copy_whose_child_folder_is_replaced_leaves_the_replacement_as_it_is() {
    let (_dir, root) = temp();
    source(&root);
    let copy = root.join("src copy");
    at(Point::Copied, {
        let copy = copy.clone();
        move || {
            replace_with_private(&copy.join("sub"), &copy.join("sub moved"));
            // And a file the copy made swapped for another's.
            fs::write(copy.join("theirs.txt"), "theirs").unwrap();
            fs::rename(copy.join("theirs.txt"), copy.join("a.txt")).unwrap();
        }
    });

    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    assert_eq!(mode(&copy.join("sub")), 0o700);
    assert_eq!(
        fs::read_to_string(copy.join("sub/private.txt")).unwrap(),
        "theirs"
    );
    // The cleanup removed what the copy made, moved or not, and kept what
    // was swapped in: so the copy's top, which holds it, stays too.
    assert_eq!(names(&copy), ["a.txt", "sub"]);
    assert_eq!(fs::read_to_string(copy.join("a.txt")).unwrap(), "theirs");
    assert_eq!(names(&copy.join("sub")), ["private.txt"]);
    assert_eq!(names(&root), ["src", "src copy"]);
}

// C2 (F13): cleanup and rollbacks keep what another process put there.

#[test]
fn a_failed_copy_keeps_a_child_inserted_into_it() {
    let (_dir, root) = temp();
    fs::create_dir(root.join("src")).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.join("src").join(name), name).unwrap();
    }
    let copy = root.join("src copy");
    // As the copy's folder is made, another process puts `b.txt` in it,
    // which the copy then finds taken.
    at(Point::Made, {
        let copy = copy.clone();
        move || fs::write(copy.join("b.txt"), "theirs").unwrap()
    });

    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // `a.txt`, which the copy made, is removed; `b.txt` is kept with what
    // was written to it, and so is the folder holding it.
    assert_eq!(names(&copy), ["b.txt"]);
    assert_eq!(fs::read_to_string(copy.join("b.txt")).unwrap(), "theirs");
}

#[test]
fn a_creation_rollback_keeps_a_folder_swapped_in() {
    let (_dir, root) = temp();
    // Once `create` has made `a`, another process moves it away, puts an
    // empty folder in its place and takes the name of the file to create
    // in the folder moved, so the creation fails and is undone.
    at(Point::Parents, {
        let root = root.clone();
        move || {
            fs::rename(root.join("a"), root.join("moved")).unwrap();
            fs::create_dir(root.join("a")).unwrap();
            fs::write(root.join("moved/b.txt"), "theirs").unwrap();
        }
    });

    let err = create_file(&root, &root, "a/b.txt").unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    assert_eq!(names(&root), ["a", "moved"]);
    assert!(names(&root.join("a")).is_empty());
    assert_eq!(names(&root.join("moved")), ["b.txt"]);
}

#[test]
fn a_creation_rollback_removes_the_folders_it_made() {
    let (_dir, root) = temp();
    at(Point::Parents, {
        let root = root.clone();
        move || fs::write(root.join("a/b/c.txt"), "theirs").unwrap()
    });
    let err = create_file(&root, &root, "a/b/c.txt").unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    // `c.txt` keeps `b` and `a`, which hold it.
    assert_eq!(names(&root.join("a/b")), ["c.txt"]);

    at(Point::Parents, {
        let root = root.clone();
        move || fs::remove_dir_all(root.join("x/y")).unwrap()
    });
    let err = create_file(&root, &root, "x/y/z.txt").unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    assert_eq!(names(&root), ["a"]);
}

#[cfg(target_os = "linux")]
#[test]
fn a_trash_rollback_keeps_a_trashinfo_swapped_in() {
    let (_dir, base) = temp();
    let project = base.join("project");
    let trash = base.join("trash");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&trash).unwrap();
    fs::write(project.join("gone.txt"), "gone").unwrap();
    let trash_dir = Root::open(&trash).unwrap().dir;
    let root = Root::open(&project).unwrap();
    let target = root.resolve(Path::new("gone.txt")).unwrap();
    // Once the `.trashinfo` is written, another process puts its own in its
    // place and moves the entry away, so the move fails and is undone.
    let info = trash.join("info/gone.txt.trashinfo");
    at(Point::Recorded, {
        let (base, info) = (base.clone(), info.clone());
        move || {
            fs::write(base.join("theirs"), "theirs").unwrap();
            fs::rename(base.join("theirs"), &info).unwrap();
            fs::rename(base.join("project/gone.txt"), base.join("gone.txt")).unwrap();
        }
    });

    let err = freedesktop::move_into(&trash_dir, &target, &target.path).unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    assert_eq!(fs::read_to_string(&info).unwrap(), "theirs");
    assert!(names(&trash.join("files")).is_empty());

    // And with nothing swapped in, the `.trashinfo` written is removed.
    fs::remove_file(&info).unwrap();
    let err = freedesktop::move_into(&trash_dir, &target, &target.path).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    assert!(names(&trash.join("info")).is_empty());
}

// The permissions cleanup gives a folder go through the handle checked to
// be it (review round 1, F1).

#[test]
#[cfg(unix)]
fn cleanup_leaves_a_folder_swapped_in_after_its_check_as_it_is() {
    // Some platforms refuse to rename a folder its owner can't write, so the
    // folder swapped in can't be put back.
    let renames = {
        let (_probe, probe) = temp();
        let folder = probe.join("a");
        fs::create_dir(&folder).unwrap();
        set_mode(&folder, 0o500);
        let result = fs::rename(&folder, probe.join("b"));
        set_mode(&probe.join(if result.is_ok() { "b" } else { "a" }), 0o700);
        match result {
            Ok(()) => true,
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => false,
            Err(err) => panic!("probe rename failed: {err}"),
        }
    };
    eprintln!("platform renames a 0o500 folder: {renames}");
    let (_dir, root) = temp();
    fs::create_dir(root.join("src")).unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(root.join("src").join(name), name).unwrap();
    }
    let copy = root.join("src copy");
    let ours = root.join("ours");
    // `b.txt` taken in the copy fails it; then, once cleanup has checked the
    // copy, another process swaps a folder of its own in for it, one its
    // owner may read but not change.
    at(Point::Made, {
        let (root, copy, ours) = (root.clone(), copy.clone(), ours.clone());
        move || {
            fs::write(copy.join("b.txt"), "theirs").unwrap();
            at(Point::Discarding, move || {
                let staged = names(&root)
                    .into_iter()
                    .find(|name| name.starts_with(".fastedit-discard-"))
                    .map(|name| root.join(name))
                    .expect("the copy is staged to be removed");
                fs::rename(&staged, &ours).unwrap();
                fs::create_dir(&staged).unwrap();
                set_mode(&staged, 0o500);
            });
        }
    });

    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
    assert_eq!(names(&ours), ["a.txt", "b.txt"]);
    if renames {
        // The folder swapped in kept its permissions, and is put back.
        assert_eq!(names(&root), ["ours", "src", "src copy"]);
        assert_eq!(mode(&copy), 0o500);
        assert!(names(&copy).is_empty());
        set_mode(&copy, 0o700);
    } else {
        // It can't be put back: it stays staged, its permissions untouched.
        let all = names(&root);
        assert_eq!(all.len(), 3, "{all:?}");
        assert!(all.contains(&"ours".to_owned()) && all.contains(&"src".to_owned()));
        let staged = root.join(
            all.iter()
                .find(|name| name.starts_with(".fastedit-discard-"))
                .expect("the swapped-in folder stays staged"),
        );
        assert_eq!(mode(&staged), 0o500);
        assert!(names(&staged).is_empty());
        set_mode(&staged, 0o700);
    }
}

#[test]
#[cfg(unix)]
fn a_closed_folder_swapped_after_its_check_keeps_its_permissions() {
    // SAFETY: always safe to call.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root reads any folder");
        return;
    }
    let (_dir, root) = temp();
    let folder = root.join("closed");
    #[cfg(target_os = "linux")]
    let ours = root.join("ours");
    fs::create_dir(&folder).unwrap();
    let parent = Root::open(&root).unwrap().dir;
    let name = OsStr::new("closed");
    let id = listing::stat_at(&parent, name).unwrap().id;
    set_mode(&folder, 0o000);
    // Once the folder is checked, another process swaps one of its own in.
    #[cfg(target_os = "linux")]
    at(Point::Owning, {
        let (folder, ours) = (folder.clone(), ours.clone());
        move || {
            fs::rename(&folder, &ours).unwrap();
            fs::create_dir(&folder).unwrap();
            set_mode(&folder, 0o000);
        }
    });

    let result = own_dir(&parent, name, Some(id));
    #[cfg(target_os = "linux")]
    {
        // Given its owner's access through the descriptor checked: the
        // folder checked, wherever it went, not the one in its place.
        assert!(reached());
        let opened = result.unwrap();
        assert_eq!(listing::identity(&opened).unwrap(), id);
        assert_eq!(mode(&ours), 0o700);
    }
    // Elsewhere no descriptor opens it, and it is never changed by name.
    #[cfg(not(target_os = "linux"))]
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(mode(&folder), 0o000);
    set_mode(&folder, 0o700);
}

// C4 (review round 1, F5): a folder made is rolled back even when it was
// never opened.

#[test]
fn a_copy_removes_a_folder_it_made_but_could_not_open() {
    let (_dir, root) = temp();
    source(&root);
    // The copy's own folder.
    fail_at(Point::Opening);
    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert_eq!(names(&root), ["src"]);

    // A folder in it.
    at(Point::Opening, || fail_at(Point::Opening));
    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert_eq!(names(&root), ["src"]);
}

#[test]
fn a_creation_removes_a_folder_it_made_but_could_not_open() {
    let (_dir, root) = temp();
    // The first folder made.
    fail_at(Point::Opening);
    let err = create_file(&root, &root, "a/b/c.txt").unwrap_err();
    assert!(reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert!(names(&root).is_empty());

    // The second, in the first.
    at(Point::Opening, || fail_at(Point::Opening));
    let err = create_file(&root, &root, "a/b/c.txt").unwrap_err();
    assert!(reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert!(names(&root).is_empty());
}

#[test]
fn a_copy_keeps_a_folder_swapped_in_for_one_it_could_not_open() {
    let (_dir, root) = temp();
    source(&root);
    let copy = root.join("src copy");
    // Once the copy's folder is made, another process swaps an empty one in
    // for it, and the open that follows fails.
    at_then_fail(Point::Opening, {
        let (copy, ours) = (copy.clone(), root.join("ours"));
        move || {
            fs::rename(&copy, &ours).unwrap();
            fs::create_dir(&copy).unwrap();
        }
    });
    let err = duplicate(&root, Path::new("src")).unwrap_err();
    assert!(reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert_eq!(names(&root), ["ours", "src", "src copy"]);
    assert!(names(&copy).is_empty());
}

#[test]
fn a_creation_keeps_a_folder_swapped_in_for_one_it_could_not_open() {
    let (_dir, root) = temp();
    at_then_fail(Point::Opening, {
        let root = root.clone();
        move || {
            fs::rename(root.join("a"), root.join("ours")).unwrap();
            fs::create_dir(root.join("a")).unwrap();
        }
    });
    let err = create_file(&root, &root, "a/b.txt").unwrap_err();
    assert!(reached());
    assert_eq!(err.raw_os_error(), Some(libc::EMFILE), "{err}");
    assert_eq!(names(&root), ["a", "ours"]);
}

// C3 (F9): the mount of an entry statx can't tell.

#[cfg(target_os = "linux")]
#[test]
fn fdinfo_names_its_mount_only_for_the_file_asked_about() {
    let text = "pos:\t0\nflags:\t0100000\nmnt_id:\t30\nino:\t5\n";
    assert_eq!(freedesktop::fdinfo_mount_in(text, 5).unwrap(), 30);
    assert!(freedesktop::fdinfo_mount_in(text, 6).is_err());
    // Before Linux 5.14 there is no `ino` to check.
    assert_eq!(
        freedesktop::fdinfo_mount_in("pos:\t0\nmnt_id:\t7\n", 5).unwrap(),
        7
    );
    assert!(freedesktop::fdinfo_mount_in("pos:\t0\n", 5).is_err());
    assert!(freedesktop::fdinfo_mount_in("mnt_id:\tx\n", 5).is_err());
}

/// Set in a child process run by [`run_child`].
#[cfg(target_os = "linux")]
const CHILD: &str = "FE_PROJECT_FS_OPS_RACE_CHILD";

#[cfg(target_os = "linux")]
const BASE: &str = "FE_PROJECT_FS_OPS_RACE_BASE";

/// Runs test `test` of this binary alone in a child process, under the
/// command `wrapper`. `None`, saying why, where the wrapper can't run it.
#[cfg(target_os = "linux")]
fn run_child(test: &str, wrapper: &[&str], envs: &[(&str, PathBuf)]) -> Option<()> {
    let exe = std::env::current_exe().unwrap();
    let (program, args) = wrapper.split_first().unwrap();
    let output = match std::process::Command::new(program)
        .args(args)
        .arg(&exe)
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(envs.iter().map(|(key, value)| (key, value)))
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            eprintln!("skipped: no bind mount, couldn't run {program}: {err}");
            return None;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() && stdout.contains("1 passed") {
        return Some(());
    }
    // A failed test exits with 101; the wrapper failing to set up, otherwise.
    if output.status.code() != Some(101) {
        eprintln!("skipped: no bind mount, {program} couldn't run the test: {stderr}");
        return None;
    }
    panic!("{test} failed in a child process:\n{stdout}\n{stderr}");
}

/// A project on a bind mount of the home trash's file system, where `statx`
/// fails or leaves the mount out: its mount is told from `/proc/self/fdinfo`,
/// so it is trashed at the top of its own mount (the walk up stopping there)
/// rather than into the home trash, a rename between mounts. Where nothing
/// tells its mount, it isn't trashed. Run where bubblewrap can make the bind
/// mount.
#[cfg(target_os = "linux")]
#[test]
fn a_bind_mounted_projects_mount_is_told_without_statx() {
    let (_dir, base) = temp();
    fs::create_dir(base.join("home")).unwrap();
    fs::create_dir_all(base.join("source/project")).unwrap();
    fs::create_dir(base.join("mount")).unwrap();
    let source = base.join("source");
    let mount = base.join("mount");
    let wrapper = [
        "bwrap",
        "--dev-bind",
        "/",
        "/",
        "--bind",
        source.to_str().unwrap(),
        mount.to_str().unwrap(),
    ];
    let envs = [("XDG_DATA_HOME", base.join("home")), (BASE, base.clone())];
    run_child(
        "fs_ops::race_tests::a_bind_mounted_projects_mount_is_told_without_statx_child",
        &wrapper,
        &envs,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_bind_mounted_projects_mount_is_told_without_statx_child() {
    use std::os::unix::fs::MetadataExt;

    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // The real trash, at the top of the bind mount, in this test's folder.
    let _real = testing::real_trash();
    let base = PathBuf::from(std::env::var_os(BASE).unwrap());
    let root = base.join("mount/project");
    let home = base.join("home");
    // The bind mount: on the home trash's device, but another mount.
    let dev = |path: &Path| fs::metadata(path).unwrap().dev();
    assert_eq!(dev(&root), dev(&home));
    let told = freedesktop::Volume::of(&Root::open(&root).unwrap().dir).unwrap();
    assert_ne!(told, freedesktop::home_trash().unwrap().volume);
    // SAFETY: always safe to call.
    let uid = unsafe { libc::getuid() };
    let trashed = base.join(format!("mount/.Trash-{uid}"));

    for fault in [Fault::StatxFails, Fault::NoMountId] {
        FAULT.set(fault);
        let name = format!("{fault:?}.txt");
        fs::write(root.join(&name), "gone").unwrap();
        let opened = Root::open(&root).unwrap();
        let target = opened.resolve(Path::new(&name)).unwrap();
        let volume = freedesktop::Volume::of(&target.parent).unwrap();
        assert_eq!(volume, told, "{fault:?}: fdinfo tells the mount statx does");
        let home_volume = freedesktop::home_trash().unwrap().volume;
        assert_ne!(volume, home_volume, "{fault:?}");
        // From the project up to the bind mount's top, and no further.
        let (_, climbed) = freedesktop::volume_trash(&target.parent, &volume).unwrap();
        assert_eq!(climbed, 1, "{fault:?}");
        trash_target(&target).unwrap();
        assert_eq!(
            fs::read_to_string(trashed.join("files").join(&name)).unwrap(),
            "gone",
            "{fault:?}"
        );
        let info = fs::read_to_string(trashed.join(format!("info/{name}.trashinfo"))).unwrap();
        assert!(info.contains(&format!("\nPath=project/{name}\n")), "{info}");
    }
    assert!(names(&home).is_empty(), "nothing went to the home trash");

    // Nothing tells the mount: said so, and nothing moves.
    FAULT.set(Fault::Unidentified);
    fs::write(root.join("kept.txt"), "kept").unwrap();
    let opened = Root::open(&root).unwrap();
    let target = opened.resolve(Path::new("kept.txt")).unwrap();
    let err = freedesktop::Volume::of(&target.parent).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported, "{err}");
    let err = trash_target(&target).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported, "{err}");
    assert_eq!(names(&root), ["kept.txt"]);
    FAULT.set(Fault::None);
}
