//! The file walk under the project search, reusable by other scans: the
//! listing walk's gitignore rules, a [`PathFilter`], regular files only, a
//! cancel flag, and files opened beneath the root without following a symlink.

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ignore::{DirEntry, WalkState};

use super::PathFilter;
use crate::listing;

/// A regular file the walk reached, opened read-only beneath the root.
#[derive(Debug)]
pub struct WalkedFile {
    /// The path as the walk reports it: under the root as given.
    pub path: PathBuf,
    /// `path` relative to the root, empty when the root is the file.
    pub rel: PathBuf,
    pub file: File,
}

/// What the walk does after a file was visited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkControl {
    Continue,
    /// Stop the walk: no more files are visited.
    Stop,
}

/// What a walk came to.
#[derive(Clone, Debug, Default)]
pub struct WalkSummary {
    /// The cancel flag was set when the walk ended.
    pub cancelled: bool,
    /// Files that couldn't be opened, with the reason.
    pub errors: Vec<(PathBuf, String)>,
}

/// What to do with a file before it is opened (crate-internal).
pub(crate) enum Pre {
    Open,
    Skip,
    Stop,
}

/// Visits the regular files under `root` that the listing walk shows and
/// `filter` lets through, each opened beneath the root following no symlink
/// (so a directory swapped for one since the walk listed it can't lead out of
/// the project), as [`super::search`] does.
///
/// With `threads == 1` files come in path order on the calling thread;
/// otherwise `visit` is called from the walker's threads, in no set order.
/// Once `cancel` is set the walk stops before the next file. Entries that
/// can't be read are skipped; files that can't be opened are in
/// [`WalkSummary::errors`].
pub fn walk_files(
    root: &Path,
    threads: usize,
    filter: &PathFilter,
    cancel: Arc<AtomicBool>,
    visit: impl Fn(WalkedFile) -> WalkControl + Sync,
) -> WalkSummary {
    let errors = Mutex::new(Vec::new());
    let visit_one = |opened: Opened| match opened {
        Ok(file) => visit(file),
        Err(error) => {
            errors.lock().unwrap().push(error);
            WalkControl::Continue
        }
    };
    if threads == 1 {
        walk_sequential(root, filter, &cancel, &|_| Pre::Open, visit_one);
    } else {
        walk_parallel(root, threads, filter, &cancel, &|_| Pre::Open, || {
            &visit_one
        });
    }
    WalkSummary {
        cancelled: cancel.load(Ordering::Relaxed),
        errors: errors.into_inner().unwrap(),
    }
}

/// A file opened, or the path that couldn't be and why.
pub(crate) type Opened = Result<WalkedFile, (PathBuf, String)>;

/// The walk on the calling thread, in path order, with `pre` asked about each
/// file (by its path relative to the root) before it is opened, and `visit`
/// given each open result.
pub(crate) fn walk_sequential(
    root: &Path,
    filter: &PathFilter,
    cancel: &AtomicBool,
    pre: &(dyn Fn(&Path) -> Pre + Sync),
    mut visit: impl FnMut(Opened) -> WalkControl,
) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let open_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let walker = walk_builder(root, filter)
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();
    for found in walker {
        // Unreadable entries are skipped, as the tree shows nothing for them.
        let Ok(found) = found else {
            continue;
        };
        if walk_entry(root, &open_root, filter, cancel, pre, &mut visit, &found) == WalkState::Quit
        {
            break;
        }
    }
}

/// The walk on the walker's `threads`, scoped to this call. `make` is called
/// once per thread for the function that thread visits with.
pub(crate) fn walk_parallel<V>(
    root: &Path,
    threads: usize,
    filter: &PathFilter,
    cancel: &AtomicBool,
    pre: &(dyn Fn(&Path) -> Pre + Sync),
    make: impl Fn() -> V + Sync,
) where
    V: FnMut(Opened) -> WalkControl + Send,
{
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let open_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let walker = walk_builder(root, filter).threads(threads).build_parallel();
    walker.run(|| {
        let mut visit = make();
        let open_root = &open_root;
        Box::new(move |found| match found {
            Ok(found) => walk_entry(root, open_root, filter, cancel, pre, &mut visit, &found),
            Err(_) => WalkState::Continue,
        })
    });
}

/// The listing walk, not entering what the filter excludes.
fn walk_builder(root: &Path, filter: &PathFilter) -> ignore::WalkBuilder {
    let root_owned = root.to_path_buf();
    let filter = filter.clone();
    listing::walk_builder(root, move |entry| {
        if filter.exclude.is_none() || entry.depth() == 0 {
            return true;
        }
        let rel = entry
            .path()
            .strip_prefix(&root_owned)
            .unwrap_or(entry.path());
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        !filter.excludes(rel, is_dir)
    })
}

fn walk_entry(
    root: &Path,
    open_root: &Path,
    filter: &PathFilter,
    cancel: &AtomicBool,
    pre: &(dyn Fn(&Path) -> Pre + Sync),
    visit: &mut impl FnMut(Opened) -> WalkControl,
    entry: &DirEntry,
) -> WalkState {
    if cancel.load(Ordering::Relaxed) {
        return WalkState::Quit;
    }
    // Regular files only: no directories, symlinks (never followed, as in
    // the tree) or FIFOs (which would block).
    if !entry.file_type().is_some_and(|kind| kind.is_file()) {
        return WalkState::Continue;
    }
    let path = entry.path();
    let rel = path.strip_prefix(root).unwrap_or(path);
    if !filter.is_empty() {
        // A file given as the root is filtered by its name.
        let name = Path::new(root.file_name().unwrap_or_default());
        let filtered = if rel.as_os_str().is_empty() {
            name
        } else {
            rel
        };
        if !filter.includes(filtered)
            || (rel.as_os_str().is_empty() && filter.excludes(filtered, false))
        {
            return WalkState::Continue;
        }
    }
    match pre(rel) {
        Pre::Open => {}
        Pre::Skip => return WalkState::Continue,
        Pre::Stop => return WalkState::Quit,
    }
    // Opened without following a symlink at any component below the root,
    // and without blocking on a FIFO, in case either replaced the file or a
    // directory above it since the walk saw it.
    let opened = match (rel.as_os_str().is_empty(), open_root.parent()) {
        // The root itself, a file: under its directory.
        (true, Some(dir)) => {
            let name = open_root.file_name().unwrap_or_default();
            listing::open_file_under(dir, Path::new(name))
        }
        _ => listing::open_file_under(open_root, rel),
    };
    let file = match opened {
        Ok(file) => file,
        Err(err) => {
            return match visit(Err((path.to_path_buf(), err.to_string()))) {
                WalkControl::Continue => WalkState::Continue,
                WalkControl::Stop => WalkState::Quit,
            };
        }
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return WalkState::Continue;
    }
    match visit(Ok(WalkedFile {
        path: path.to_path_buf(),
        rel: rel.to_path_buf(),
        file,
    })) {
        WalkControl::Continue => WalkState::Continue,
        WalkControl::Stop => WalkState::Quit,
    }
}
