use std::{
    collections::HashSet,
    fs,
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{
    Event, EventKind,
    event::{CreateKind, Flag, ModifyKind},
};

use super::*;
use crate::watcher::Batch;

/// Applies batches from `rx` until `done` holds for the tree, checking that
/// no batch repeats a path. Fails after a generous deadline; how long the
/// events take isn't asserted.
fn wait_for(
    tree: &mut FileTree,
    rx: &mpsc::Receiver<Vec<FsChange>>,
    what: &str,
    done: impl Fn(&FileTree) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(tree) {
        let left = deadline.saturating_duration_since(Instant::now());
        let batch = rx
            .recv_timeout(left)
            .unwrap_or_else(|err| panic!("no batch showing {what}: {err}"));
        let unique: HashSet<_> = batch.iter().collect();
        assert_eq!(
            unique.len(),
            batch.len(),
            "a batch repeats a path: {batch:?}"
        );
        tree.apply(&batch);
    }
}

#[test]
fn watcher_batches_create_rename_delete() {
    let (_dir, root) = temp();
    make(&root, &["src/main.rs"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "src"));
    let (tx, rx) = mpsc::channel();
    let _watcher = ProjectWatcher::new(&root, move |batch| {
        let _ = tx.send(batch);
    })
    .unwrap();

    fs::write(root.join("src/new.rs"), "fn main() {}").unwrap();
    wait_for(&mut tree, &rx, "the created file", |tree| {
        tree.find("src/new.rs").is_some()
    });
    assert_eq!(layout(&tree), ["v src", "  main.rs", "  new.rs"]);

    fs::rename(root.join("src/new.rs"), root.join("src/renamed.rs")).unwrap();
    wait_for(&mut tree, &rx, "the rename", |tree| {
        tree.find("src/new.rs").is_none() && tree.find("src/renamed.rs").is_some()
    });
    assert_eq!(layout(&tree), ["v src", "  main.rs", "  renamed.rs"]);

    fs::remove_file(root.join("src/renamed.rs")).unwrap();
    wait_for(&mut tree, &rx, "the deletion", |tree| {
        tree.find("src/renamed.rs").is_none()
    });
    assert_eq!(layout(&tree), ["v src", "  main.rs"]);

    // Within a batch each path, and the rescan marker, appears once.
    let event = |kind, path: &str| Ok(Event::new(kind).add_path(root.join(path)));
    let mut batch = Batch::default();
    batch.add(event(EventKind::Create(CreateKind::File), "a"));
    batch.add(event(EventKind::Modify(ModifyKind::Any), "a"));
    batch.add(event(EventKind::Create(CreateKind::File), "b"));
    batch.add(Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan)));
    batch.add(Err(notify::Error::generic("dropped")));
    batch.add(event(EventKind::Modify(ModifyKind::Any), "b"));
    assert_eq!(
        batch.changes,
        [
            FsChange::Path(root.join("a")),
            FsChange::Path(root.join("b")),
            FsChange::Rescan,
        ]
    );
}
