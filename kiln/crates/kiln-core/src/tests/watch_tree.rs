// SPDX-License-Identifier: Apache-2.0

//! [`Trees`], the tree watcher's bookkeeping, re-stated without events, as
//! when FSEvents drops them: a tree too big to read whole, and links to
//! extensions outside the watched root.

#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::{
    fs,
    path::{Path, PathBuf},
};

use notify::RecursiveMode;
#[cfg(unix)]
use notify::{Event, EventKind, event::ModifyKind};
use tempfile::TempDir;

use crate::watch_tree::Trees;

/// What a test's watch was asked to watch.
type Watched = Vec<(PathBuf, RecursiveMode)>;

fn watching(watched: &mut Watched) -> impl FnMut(&Path, RecursiveMode) -> notify::Result<()> + '_ {
    |path, mode| {
        watched.push((path.to_owned(), mode));
        Ok(())
    }
}

/// Rewrites `path` at a new length, so its stamp differs.
#[cfg(unix)]
fn rewrite(path: &Path) {
    let len = fs::read(path).map_or(0, |text| text.len());
    fs::write(path, "x".repeat(len + 1)).unwrap();
}

#[test]
fn a_tree_past_the_limit_is_reported_on_every_restat() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("extensions");
    for name in ["a", "b", "c", "d"] {
        fs::create_dir_all(root.join(name)).unwrap();
        fs::write(root.join(name).join("extension.toml"), "").unwrap();
    }
    let mut watched = Watched::new();
    let mut watch = watching(&mut watched);
    let mut whole = Trees::with_limit(vec![root.clone()], 100);
    assert_eq!(whole.register(&mut watch), std::slice::from_ref(&root));
    assert_eq!(whole.reconcile(&mut watch), Vec::<PathBuf>::new());

    let mut truncated = Trees::with_limit(vec![root.clone()], 3);
    assert_eq!(truncated.register(&mut watch), std::slice::from_ref(&root));
    // Nothing changed, but a change past the limit couldn't be seen.
    assert_eq!(truncated.reconcile(&mut watch), std::slice::from_ref(&root));
    assert_eq!(truncated.reconcile(&mut watch), std::slice::from_ref(&root));
}

#[test]
#[cfg(unix)]
fn links_to_extensions_outside_the_root_are_followed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("extensions");
    fs::create_dir_all(root.join("file-linked")).unwrap();
    let outside = dir.path().join("checkout");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("extension.toml"), "").unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("extension.toml"), "").unwrap();
    symlink(&outside, root.join("dir-linked")).unwrap();
    symlink(
        elsewhere.join("extension.toml"),
        root.join("file-linked").join("extension.toml"),
    )
    .unwrap();
    // A loop ends.
    symlink(&root, root.join("loop")).unwrap();

    let mut watched = Watched::new();
    let mut trees = Trees::new(vec![root.clone()]);
    assert_eq!(
        trees.register(&mut watching(&mut watched)),
        std::slice::from_ref(&root)
    );
    let outside = crate::platform::canonicalize(&outside).unwrap();
    let elsewhere = crate::platform::canonicalize(&elsewhere).unwrap();
    assert!(watched.contains(&(root.clone(), RecursiveMode::Recursive)));
    assert!(watched.contains(&(outside.clone(), RecursiveMode::Recursive)));
    assert!(watched.contains(&(elsewhere.clone(), RecursiveMode::NonRecursive)));

    let mut watch = watching(&mut watched);
    assert_eq!(trees.reconcile(&mut watch), Vec::<PathBuf>::new());
    rewrite(&outside.join("extension.toml"));
    assert_eq!(trees.reconcile(&mut watch), std::slice::from_ref(&root));
    assert_eq!(trees.reconcile(&mut watch), Vec::<PathBuf>::new());
    rewrite(&elsewhere.join("extension.toml"));
    assert_eq!(trees.reconcile(&mut watch), std::slice::from_ref(&root));

    // Events name the targets as they are.
    let event = |path: PathBuf| Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path));
    let batch = [event(outside.join("extension.toml"))];
    assert_eq!(trees.batch(&batch, &mut watch), std::slice::from_ref(&root));
    let batch = [event(elsewhere.join("extension.toml"))];
    assert_eq!(trees.batch(&batch, &mut watch), std::slice::from_ref(&root));
    let batch = [event(elsewhere.join("other.toml"))];
    assert_eq!(trees.batch(&batch, &mut watch), Vec::<PathBuf>::new());
}
