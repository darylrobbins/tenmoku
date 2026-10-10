//! `fe_project::open_file_under`, which extraction opens files through: no
//! symlink is followed at any component of the path below the root,
//! whenever it was swapped in.

use std::{
    fs,
    io::{ErrorKind, Read},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use fe_nav::{Skipped, extract};
use fe_project::open_file_under;

/// A project at `<dir>/project`, with `src/a/x.rs`, and `<dir>/outside`,
/// with an `x.rs` of its own. Returns the canonical project root and the
/// outside directory.
fn project(dir: &Path) -> (PathBuf, PathBuf) {
    let dir = fs::canonicalize(dir).unwrap();
    let (root, outside) = (dir.join("project"), dir.join("outside"));
    fs::create_dir_all(root.join("src/a")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("src/a/x.rs"), "fn inside() {}\n").unwrap();
    fs::write(outside.join("x.rs"), "fn outside() {}\n").unwrap();
    (root, outside)
}

fn contents(root: &Path, rel: &str) -> std::io::Result<String> {
    let mut text = String::new();
    open_file_under(root, Path::new(rel))?.read_to_string(&mut text)?;
    Ok(text)
}

fn error(root: &Path, rel: impl AsRef<Path>) -> ErrorKind {
    open_file_under(root, rel.as_ref()).unwrap_err().kind()
}

#[test]
fn a_component_swapped_for_a_symlink_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (root, outside) = project(dir.path());
    assert_eq!(contents(&root, "src/a/x.rs").unwrap(), "fn inside() {}\n");
    assert_eq!(contents(&root, "./src/a/x.rs").unwrap(), "fn inside() {}\n");

    // Each component in turn swapped for a symlink to its like outside.
    fs::create_dir_all(outside.join("src/a")).unwrap();
    fs::copy(outside.join("x.rs"), outside.join("src/a/x.rs")).unwrap();
    for swapped in ["src", "src/a", "src/a/x.rs"] {
        let (path, aside) = (root.join(swapped), root.join("aside"));
        fs::rename(&path, &aside).unwrap();
        symlink(outside.join(swapped), &path).unwrap();
        assert_eq!(error(&root, "src/a/x.rs"), ErrorKind::NotFound, "{swapped}");
        fs::remove_file(&path).unwrap();
        fs::rename(&aside, &path).unwrap();
        assert!(contents(&root, "src/a/x.rs").is_ok(), "{swapped} back");
    }
}

#[test]
fn paths_not_below_the_root_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (root, outside) = project(dir.path());
    for rel in ["../outside/x.rs", "src/../src/a/x.rs", "src/a/..", "", "."] {
        assert_eq!(error(&root, rel), ErrorKind::InvalidInput, "{rel:?}");
    }
    for absolute in [root.join("src/a/x.rs"), outside.join("x.rs")] {
        assert_eq!(error(&root, &absolute), ErrorKind::InvalidInput);
    }
}

#[test]
fn a_fifo_opens_without_waiting_for_a_writer() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = project(dir.path());
    let status = Command::new("mkfifo")
        .arg(root.join("src/a/fifo.rs"))
        .status()
        .unwrap();
    assert!(status.success());
    let file = open_file_under(&root, Path::new("src/a/fifo.rs")).unwrap();
    assert!(!file.metadata().unwrap().is_file());
    assert_eq!(
        extract(&root, Path::new("src/a/fifo.rs")).unwrap_err(),
        Skipped::Unreadable(ErrorKind::InvalidInput)
    );
}

#[test]
fn extraction_never_reads_through_a_directory_being_swapped() {
    let dir = tempfile::tempdir().unwrap();
    let (root, outside) = project(dir.path());
    let (a, aside) = (root.join("src/a"), root.join("src/a.aside"));
    let rel = Path::new("src/a/x.rs");
    let done = AtomicBool::new(false);
    // The handshake with the swapper: `hold` asks it to stop between swaps,
    // and it sets `parked` once it has, with `src/a` in place.
    let (hold, parked) = (AtomicBool::new(false), AtomicBool::new(false));
    thread::scope(|scope| {
        // Stops the swapping however this thread leaves the scope.
        let _stop = Stop(&done);
        let swapper = scope.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if hold.load(Ordering::Acquire) {
                    parked.store(true, Ordering::Release);
                    thread::yield_now();
                    continue;
                }
                parked.store(false, Ordering::Release);
                fs::rename(&a, &aside).unwrap();
                symlink(&outside, &a).unwrap();
                fs::remove_file(&a).unwrap();
                fs::rename(&aside, &a).unwrap();
            }
        });
        // Safety: extracts racing the swaps never read from outside.
        for _ in 0..500 {
            match extract(&root, rel) {
                Ok(file) => assert_eq!(file.symbols[0].name, "inside"),
                Err(skipped) => assert_eq!(skipped, Skipped::Unreadable(ErrorKind::NotFound)),
            }
        }
        // Liveness: an extract with the directory known to be in place.
        hold.store(true, Ordering::Release);
        while !parked.load(Ordering::Acquire) {
            assert!(!swapper.is_finished(), "the swapper died");
            thread::yield_now();
        }
        let file = extract(&root, rel).expect("directory in place");
        assert_eq!(file.symbols[0].name, "inside");
        hold.store(false, Ordering::Release);
    });
}

/// Sets its flag when dropped.
struct Stop<'a>(&'a AtomicBool);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// fe-nav opens files only through fe-project, so it needs no `unsafe` of
/// its own.
#[test]
fn fe_nav_has_no_unsafe() {
    let src = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                assert!(!text.contains("unsafe"), "{}", path.display());
            }
        }
    }
}
