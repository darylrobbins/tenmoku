//! What `FinderIndex::apply` and `finish` report a batch changed.

use std::sync::{Arc, atomic::AtomicBool};

use super::*;

/// Walks `root` into a new index and finishes it.
fn walked(root: &Path) -> FinderIndex {
    let mut index = FinderIndex::new(root);
    assert!(FinderIndex::walk(
        root,
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch)
    ));
    assert_eq!(index.finish(), FileSetDelta::default());
    index
}

fn paths(rels: &[&str]) -> Vec<PathBuf> {
    rels.iter().map(PathBuf::from).collect()
}

#[test]
fn apply_reports_present_and_removed() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "src/kept.rs",
            "src/edited.rs",
            "src/deleted.rs",
            "gone/a.rs",
            "gone/deep/b.rs",
            "build/",
        ],
    );
    fs::write(root.join(".gitignore"), "build/\n").unwrap();
    let mut index = walked(&root);

    make(
        &root,
        &[
            "src/created.rs",
            "fresh/one.rs",
            "fresh/sub/two.rs",
            "build/ignored.rs",
        ],
    );
    fs::write(root.join("src/edited.rs"), "fn edited() {}\n").unwrap();
    fs::remove_file(root.join("src/deleted.rs")).unwrap();
    fs::remove_dir_all(root.join("gone")).unwrap();
    // Out of order and repeated, as a watcher can report them; `gone` is
    // reported by its own path only.
    let delta = index.apply(&changes(
        &root,
        &[
            "src/edited.rs",
            "src/created.rs",
            "fresh/sub/two.rs",
            "fresh",
            "src/deleted.rs",
            "gone",
            "build/ignored.rs",
            "build",
            "src/edited.rs",
            "fresh/one.rs",
        ],
    ));
    assert_eq!(
        delta,
        FileSetDelta {
            present: paths(&[
                "fresh/one.rs",
                "fresh/sub/two.rs",
                "src/created.rs",
                "src/edited.rs",
            ]),
            removed: paths(&["gone/a.rs", "gone/deep/b.rs", "src/deleted.rs"]),
            rescanned: false,
        }
    );
    assert!(index.contains("fresh/sub/two.rs"));
    assert!(!index.contains("gone/a.rs"));
    assert!(!index.contains("build/ignored.rs"));

    // A batch that changes nothing reports nothing.
    let delta = index.apply(&changes(&root, &["build/ignored.rs", "missing.rs"]));
    assert_eq!(delta, FileSetDelta::default());
}

#[test]
fn deferred_and_rescan_deltas() {
    let (_dir, root) = temp();
    make(&root, &["a.rs", "b.rs", "dir/c.rs"]);

    // While building, `apply` reports nothing and `finish` reports it all,
    // including a removal of a path the build had already handed over.
    let mut index = FinderIndex::new(&root);
    index.extend(paths(&["a.rs", "b.rs"]));
    make(&root, &["new.rs"]);
    fs::remove_file(root.join("b.rs")).unwrap();
    let during = index.apply(&changes(&root, &["new.rs", "b.rs"]));
    assert_eq!(during, FileSetDelta::default());
    // The build's last batch, listed before the changes.
    index.extend(paths(&["b.rs", "dir/c.rs"]));
    assert_eq!(
        index.finish(),
        FileSetDelta {
            present: paths(&["new.rs"]),
            removed: paths(&["b.rs"]),
            rescanned: false,
        }
    );
    assert_eq!(index.finish(), FileSetDelta::default());

    // A rescan: what's gone is removed, and every path is present.
    make(&root, &["unreported.rs"]);
    fs::write(root.join("a.rs"), "changed").unwrap();
    fs::remove_dir_all(root.join("dir")).unwrap();
    let delta = index.apply(&[FsChange::Path(root.join("a.rs")), FsChange::Rescan]);
    assert_eq!(
        delta,
        FileSetDelta {
            present: paths(&["a.rs", "new.rs", "unreported.rs"]),
            removed: paths(&["dir/c.rs"]),
            rescanned: true,
        }
    );

    // A rescan that came while building is walked, and reported, by
    // `finish`.
    let mut index = FinderIndex::new(&root);
    index.extend(paths(&["a.rs", "stale.rs"]));
    assert_eq!(index.apply(&[FsChange::Rescan]), FileSetDelta::default());
    assert_eq!(
        index.finish(),
        FileSetDelta {
            present: paths(&["a.rs", "new.rs", "unreported.rs"]),
            removed: paths(&["stale.rs"]),
            rescanned: true,
        }
    );
}

#[test]
fn a_path_added_while_building_then_handed_over_is_reported_removed() {
    let (_dir, root) = temp();
    make(&root, &["a.rs", "x.rs"]);
    for rescan in [false, true] {
        let mut index = FinderIndex::new(&root);
        index.extend(paths(&["a.rs"]));
        // Added by a batch before the walk reaches it, then handed over by
        // the build too: from then on it's indexed, and its removal counts.
        make(&root, &["d/new.rs"]);
        index.apply(&changes(&root, &["d/new.rs"]));
        index.extend(paths(&["d/new.rs", "x.rs"]));
        fs::remove_file(root.join("d/new.rs")).unwrap();
        let mut batch = changes(&root, &["d/new.rs"]);
        if rescan {
            batch.push(FsChange::Rescan);
        }
        assert_eq!(index.apply(&batch), FileSetDelta::default());
        let delta = index.finish();
        assert_eq!(delta.removed, paths(&["d/new.rs"]), "rescan: {rescan}");
        assert_eq!(delta.rescanned, rescan);
    }
}
