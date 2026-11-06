use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::{listing, *};

mod finder;
mod finder_backpressure;
mod finder_delta;
mod finder_followups;
mod finder_walk_safety;
mod large;
mod search;
mod tree;
mod tree_root_loss;
mod watcher;
pub(crate) mod watcher_recovery;

/// A temporary directory and its canonical path (on macOS `/var` is a link to
/// `/private/var`, and watchers report the latter).
fn temp() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dunce::canonicalize(dir.path()).unwrap();
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

/// Moves directory `path` aside and puts a link to `target` in its place.
fn swap_for_link(path: &Path, target: &Path) {
    fs::rename(path, aside(path)).unwrap();
    dir_link(target, path);
}

/// Undoes [`swap_for_link`].
fn swap_back(path: &Path) {
    remove_link(path);
    fs::rename(aside(path), path).unwrap();
}

/// Relative path `path`, written with `/`, as the finder and search spell
/// it: with the platform's separator (`\` on Windows).
fn native(path: &str) -> String {
    path.replace('/', std::path::MAIN_SEPARATOR_STR)
}

/// A relative path as the finder or search spells it, written with `/`, so
/// tests compare it alike on every platform.
fn slashed(path: String) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        path
    } else {
        path.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

/// Links folder `link` to folder `target`: a symlink, or on Windows a
/// junction, which needs no privilege and is a link all the same.
fn dir_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // `mklink` takes neither `\\?\` paths nor `/` separators.
        let plain = |path: &Path| {
            let text = path.to_str().unwrap();
            text.strip_prefix(r"\\?\").unwrap_or(text).replace('/', r"\")
        };
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &plain(link), &plain(target)])
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }
}

/// Removes link `link` made by [`dir_link`], never what it leads to: on
/// Windows a junction is a folder's entry.
fn remove_link(link: &Path) {
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
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

/// `path`, absolute, as a path relative to the current directory (which no
/// test changes); `None` where none leads there, as from another Windows
/// drive.
fn relative(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};

    let cwd = std::env::current_dir().unwrap();
    // A drive, however it's spelled (`C:` or `\\?\C:`).
    let drive = |path: &Path| match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => Some(drive.to_ascii_uppercase()),
            _ => None,
        },
        _ => None,
    };
    if drive(&cwd) != drive(path) {
        return None;
    }
    let normal = |component: &Component<'_>| matches!(component, Component::Normal(_));
    let ups = cwd.components().filter(normal).count();
    let mut rel: PathBuf = std::iter::repeat_n("..", ups).collect();
    rel.extend(path.components().filter(normal));
    Some(rel)
}

fn aside(path: &Path) -> PathBuf {
    let mut aside = path.as_os_str().to_os_string();
    aside.push(".aside");
    aside.into()
}
