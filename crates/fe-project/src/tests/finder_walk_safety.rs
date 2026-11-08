//! The finder's walks keep `FileTree`'s file system protections: ignore files
//! are read only when they are regular files, opened without following
//! symlinks or waiting on a FIFO, and directories are opened through their
//! parent's handle, never through a symlink.

use std::sync::{Arc, atomic::AtomicBool};
#[cfg(unix)]
use std::{
    os::unix::{ffi::OsStrExt, fs::symlink},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use super::*;

/// How long a walk may take before it counts as blocked.
#[cfg(unix)]
const BLOCKED: Duration = Duration::from_secs(5);

/// Walks `root` into a new index and finishes it.
fn walked(root: &Path) -> FinderIndex {
    let mut index = FinderIndex::new(root);
    assert!(FinderIndex::walk(
        root,
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch)
    ));
    index.finish();
    index
}

/// Every indexed path, in path order.
fn all(index: &mut FinderIndex) -> Vec<String> {
    index
        .query("", usize::MAX)
        .into_iter()
        .map(|found| slashed(found.path))
        .collect()
}

/// Runs `f` on another thread and returns its result, failing the test
/// (rather than hanging it) if it takes longer than [`BLOCKED`]. A blocked
/// thread is left behind.
#[cfg(unix)]
fn bounded<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(BLOCKED)
        .unwrap_or_else(|_| panic!("{what} was blocked for {BLOCKED:?}"))
}

/// Whether the tree, with every directory expanded, shows `path` as an
/// unignored file.
#[cfg(unix)]
fn tree_shows(root: &Path, path: &str) -> bool {
    let mut tree = FileTree::open(root, TreeOptions::default()).unwrap();
    let rel = Path::new(path);
    for dir in rel
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if !dir.as_os_str().is_empty() {
            tree.expand(id(&tree, dir.to_str().unwrap()));
        }
    }
    tree.find(path).is_some_and(|node| {
        tree.rows(0..tree.row_count())
            .any(|row| row.id == node && !row.ignored && row.kind == EntryKind::File)
    })
}

#[test]
#[cfg(unix)]
fn walk_ignores_a_symlinked_gitignore() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "rules.txt",
            "hidden.rs",
            "kept.rs",
            "sub/hidden.rs",
            "sub/rules.txt",
        ],
    );
    fs::write(root.join("rules.txt"), "hidden.rs\n").unwrap();
    fs::write(root.join("sub/rules.txt"), "hidden.rs\n").unwrap();
    symlink(root.join("rules.txt"), root.join(".gitignore")).unwrap();
    symlink("rules.txt", root.join("sub/.ignore")).unwrap();
    let expected = [
        "hidden.rs",
        "kept.rs",
        "rules.txt",
        "sub/hidden.rs",
        "sub/rules.txt",
    ];
    // The tree doesn't obey them either.
    assert!(tree_shows(&root, "hidden.rs"));
    assert!(tree_shows(&root, "sub/hidden.rs"));

    let mut index = walked(&root);
    assert_eq!(all(&mut index), expected);
    index.apply(&[FsChange::Rescan]);
    assert_eq!(all(&mut index), expected);

    // A new directory walked by `apply`.
    make(&root, &["fresh/hidden.rs", "fresh/rules.txt"]);
    fs::write(root.join("fresh/rules.txt"), "hidden.rs\n").unwrap();
    symlink("rules.txt", root.join("fresh/.gitignore")).unwrap();
    index.apply(&changes(&root, &["fresh"]));
    assert!(all(&mut index).contains(&"fresh/hidden.rs".to_owned()));
    assert!(tree_shows(&root, "fresh/hidden.rs"));
}

#[test]
#[cfg(unix)]
fn a_fifo_ignore_file_does_not_block_a_cancelled_walk() {
    let (dir, root) = temp();
    make(&root, &["top.rs", "sub/a.rs", "sub/deep/b.rs"]);
    let fifo = std::ffi::CString::new(root.join("sub/.ignore").as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo` is a NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);

    // A walk runs to the end, or stops once cancelled, without waiting on
    // the FIFO.
    let cancel = Arc::new(AtomicBool::new(false));
    let walk = {
        let (root, cancel) = (root.clone(), cancel.clone());
        thread::spawn(move || {
            let mut paths = Vec::new();
            let done = FinderIndex::walk(&root, cancel, |batch| paths.extend(batch));
            (done, paths)
        })
    };
    let (done, paths) = bounded("the walk", move || {
        thread::sleep(Duration::from_millis(50));
        cancel.store(true, Ordering::Relaxed);
        walk.join().unwrap()
    });
    if done {
        let mut paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        paths.sort();
        assert!(paths.contains(&"sub/a.rs".to_owned()), "{paths:?}");
        assert!(paths.contains(&"sub/deep/b.rs".to_owned()), "{paths:?}");
    }

    // So do the walks `apply` makes, on its caller's thread: of the root,
    // and of a new directory.
    let mut index = {
        let root = root.clone();
        bounded("the first walk", move || walked(&root))
    };
    assert!(all(&mut index).contains(&"sub/deep/b.rs".to_owned()));
    let index = bounded("a rescan", move || {
        index.apply(&[FsChange::Rescan]);
        index
    });
    fs::rename(root.join("sub"), root.join("moved")).unwrap();
    let changed = changes(&root, &["sub", "moved"]);
    let mut index = bounded("a new directory's walk", move || {
        let mut index = index;
        index.apply(&changed);
        index
    });
    let paths = all(&mut index);
    assert!(paths.contains(&"moved/deep/b.rs".to_owned()), "{paths:?}");
    assert!(
        !paths.iter().any(|path| path.starts_with("sub/")),
        "{paths:?}"
    );
    drop(dir);
}

#[test]
fn walk_does_not_follow_a_directory_symlink() {
    let (_dir, root) = temp();
    let (_outside_dir, outside) = temp();
    make(&outside, &["secret.rs", "deep/secret.rs"]);
    make(&root, &["a.rs", "sub/b.rs"]);
    dir_link(&outside, &root.join("link"));
    dir_link(&outside, &root.join("sub/link"));
    let expected = ["a.rs", "sub/b.rs"];

    let mut index = walked(&root);
    assert_eq!(all(&mut index), expected);
    index.apply(&[FsChange::Rescan]);
    assert_eq!(all(&mut index), expected);

    // A new directory holding one, walked by `apply`, and the links
    // themselves reported.
    make(&root, &["fresh/c.rs"]);
    dir_link(&outside, &root.join("fresh/link"));
    index.apply(&changes(
        &root,
        &["fresh", "fresh/link", "link", "sub/link"],
    ));
    assert_eq!(all(&mut index), ["a.rs", "fresh/c.rs", "sub/b.rs"]);

    // A walk of a root that is itself reached through a symlink indexes the
    // directory it leads to, as the tree shows it, and nothing outside.
    let (_links, links) = temp();
    dir_link(&root, &links.join("project"));
    let mut index = walked(&links.join("project"));
    assert_eq!(all(&mut index), ["a.rs", "fresh/c.rs", "sub/b.rs"]);

    // The root itself swapped for a symlink: a rescan doesn't follow it, nor
    // does one `finish` makes for a rescan that came while building.
    let mut index = walked(&root);
    let mut building = FinderIndex::new(&root);
    swap_for_link(&root, &outside);
    index.apply(&[FsChange::Rescan]);
    assert_eq!(all(&mut index), Vec::<String>::new());
    building.apply(&[FsChange::Rescan]);
    building.finish();
    assert_eq!(all(&mut building), Vec::<String>::new());
    swap_back(&root);
    index.apply(&[FsChange::Rescan]);
    assert_eq!(all(&mut index), ["a.rs", "fresh/c.rs", "sub/b.rs"]);
}

#[test]
// Counts this process's open descriptors in /dev/fd.
#[cfg(unix)]
fn a_deep_tree_is_walked_without_a_handle_per_level() {
    // Levels of `d`, each with a file and empty sibling directories, which
    // keep their parent wanted while the walk goes deeper first.
    const DEPTH: usize = 100;
    let (_dir, root) = temp();
    let mut expected = Vec::new();
    let mut rel = PathBuf::new();
    for _ in 0..DEPTH {
        for ix in 0..40 {
            fs::create_dir_all(root.join(rel.join(format!("s{ix}")))).unwrap();
        }
        let file = rel.join("f.rs");
        fs::write(root.join(&file), "").unwrap();
        expected.push(file.to_str().unwrap().to_owned());
        rel.push("d");
    }
    expected.sort();

    let open = || fs::read_dir("/dev/fd").unwrap().count();
    let before = open();
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let stop = stop.clone();
        thread::spawn(move || {
            let mut peak = 0;
            while !stop.load(Ordering::Relaxed) {
                peak = peak.max(open());
            }
            peak
        })
    };
    let mut index = walked(&root);
    index.apply(&[FsChange::Rescan]);
    index.apply(&changes(&root, &["d"]));
    stop.store(true, Ordering::Relaxed);
    let peak = sampler.join().unwrap();
    assert_eq!(all(&mut index), expected);
    // The walk keeps 32 directories open for their subdirectories, plus
    // those being read and a few more for a moment on each thread: well
    // under one per level, even with other tests' descriptors.
    assert!(
        peak.saturating_sub(before) < 80,
        "{before} open before, {peak} at the peak"
    );
}

#[test]
// Windows won't rename a folder while a handle holds it (or what is in it), as this race must.
#[cfg(unix)]
fn a_directory_swapped_for_a_symlink_mid_walk_is_not_followed() {
    // `sub` is swapped for a symlink to a directory outside the project and
    // back, over and over, while the index walks: every walk, a rescan and
    // `apply`'s walk of `sub` itself. Files outside are never indexed, under
    // `sub` or anywhere.
    let (_dir, root) = temp();
    let (_outside_dir, outside) = temp();
    make(&root, &["a.rs", "sub/b.rs", "sub/deeper/c.rs"]);
    make(&outside, &["target/secret.rs", "target/deeper/secret.rs"]);
    let sub = root.join("sub");
    let (target, aside) = (outside.join("target"), outside.join("aside"));
    let stop = Arc::new(AtomicBool::new(false));
    let swap_count = Arc::new(AtomicU64::new(0));
    let swapper = {
        let (stop, swap_count) = (stop.clone(), swap_count.clone());
        thread::spawn(move || {
            let mut swaps = 0_u64;
            while !stop.load(Ordering::Relaxed) {
                fs::rename(&sub, &aside).unwrap();
                dir_link(&target, &sub);
                remove_link(&sub);
                fs::rename(&aside, &sub).unwrap();
                swaps += 1;
                swap_count.store(swaps, Ordering::Relaxed);
            }
            swaps
        })
    };
    let leaked = |index: &mut FinderIndex| {
        let paths = all(index);
        paths
            .iter()
            .any(|path| path.contains("secret"))
            .then_some(paths)
    };
    let started = std::time::Instant::now();
    let mut walks = 0;
    let mut found = None;
    // Walk until the minimums below are met (and for at least 1 s), however
    // slow the machine is; the deadline only stops a swapper that is stuck.
    while found.is_none()
        && started.elapsed() < Duration::from_secs(30)
        && (started.elapsed() < Duration::from_secs(1)
            || walks <= 10
            || swap_count.load(Ordering::Relaxed) <= 100)
    {
        let mut index = walked(&root);
        found = found.or_else(|| leaked(&mut index));
        index.apply(&[FsChange::Rescan]);
        found = found.or_else(|| leaked(&mut index));
        index.apply(&changes(&root, &["sub"]));
        found = found.or_else(|| leaked(&mut index));
        walks += 1;
    }
    stop.store(true, Ordering::Relaxed);
    let swaps = swapper.join().unwrap();
    assert_eq!(found, None, "after {walks} rounds and {swaps} swaps");
    assert!(swaps > 100 && walks > 10, "{swaps} swaps, {walks} rounds");
}
