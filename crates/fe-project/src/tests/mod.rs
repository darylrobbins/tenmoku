use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::{listing, *};

mod finder;
mod large;
mod tree;
mod watcher;

/// A temporary directory and its canonical path (on macOS `/var` is a link to
/// `/private/var`, and watchers report the latter).
fn temp() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    (dir, path)
}

/// Creates each path under `root`: a trailing `/` makes a directory, anything
/// else an empty file (with its parent directories).
fn make(root: &Path, paths: &[&str]) {
    for path in paths {
        let full = root.join(path);
        if path.ends_with('/') {
            fs::create_dir_all(full).unwrap();
        } else {
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "").unwrap();
        }
    }
}

/// One line per row: indented two spaces per depth, `>` before a collapsed
/// directory and `v` before an expanded one, then `(ignored)` and `(error)`
/// markers.
fn layout(tree: &FileTree) -> Vec<String> {
    tree.rows(0..tree.row_count())
        .map(|row| describe(&row))
        .collect()
}

fn describe(row: &Row<'_>) -> String {
    let marker = match (row.kind, row.expanded) {
        (EntryKind::Dir, true) => "v ",
        (EntryKind::Dir, false) => "> ",
        (EntryKind::Symlink, _) => "@ ",
        (EntryKind::File, _) => "",
    };
    let mut line = format!(
        "{}{marker}{}",
        "  ".repeat(row.depth),
        row.name.to_string_lossy()
    );
    if row.ignored {
        line.push_str(" (ignored)");
    }
    if row.error.is_some() {
        line.push_str(" (error)");
    }
    line
}

fn id(tree: &FileTree, path: &str) -> NodeId {
    tree.find(path)
        .unwrap_or_else(|| panic!("{path} is not loaded"))
}

fn changes(root: &Path, paths: &[&str]) -> Vec<FsChange> {
    paths
        .iter()
        .map(|path| FsChange::Path(root.join(path)))
        .collect()
}

/// Runs `f` with `hook` called at each [`listing::Point`] it reaches.
fn with_hook<T>(hook: impl FnMut(listing::Point) + 'static, f: impl FnOnce() -> T) -> T {
    listing::HOOK.set(Some(Box::new(hook)));
    let result = f();
    listing::HOOK.set(None);
    result
}

/// Moves directory `path` aside and puts a symlink to `target` in its place.
fn swap_for_link(path: &Path, target: &Path) {
    fs::rename(path, aside(path)).unwrap();
    std::os::unix::fs::symlink(target, path).unwrap();
}

/// Undoes [`swap_for_link`].
fn swap_back(path: &Path) {
    fs::remove_file(path).unwrap();
    fs::rename(aside(path), path).unwrap();
}

/// Moves directory `path` back from where [`aside`] put it.
fn swap_back_dir(path: &Path) {
    fs::rename(aside(path), path).unwrap();
}

/// Moves directory `path` aside and makes an empty one in its place.
fn swap_for_dir(path: &Path) {
    fs::rename(path, aside(path)).unwrap();
    fs::create_dir(path).unwrap();
}

fn aside(path: &Path) -> PathBuf {
    let mut aside = path.as_os_str().to_os_string();
    aside.push(".aside");
    aside.into()
}
