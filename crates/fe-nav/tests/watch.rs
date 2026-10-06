//! Keeping a `SymbolIndex` current from watcher batches: `plan`, then
//! `extract_all` off the index, then `update`. The batches are made up
//! here, as a `ProjectWatcher` would send them, so nothing waits on file
//! system events.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use fe_nav::{BuildOptions, Reindex, SymbolIndex, build, extract_all};
use fe_project::FsChange;

/// A temporary project and its canonical root, as watchers report it.
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn write(root: &Path, rel: &str, text: impl AsRef<[u8]>) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Builds an index of `root` and finishes it, with nothing to redo.
fn built(root: &Path) -> SymbolIndex {
    let mut index = SymbolIndex::new(root);
    build(
        root,
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch),
    );
    assert_eq!(index.finish(), Reindex::default());
    index
}

fn changes(root: &Path, rels: &[&str]) -> Vec<FsChange> {
    rels.iter()
        .map(|rel| FsChange::Path(root.join(rel)))
        .collect()
}

fn paths(rels: &[&str]) -> Vec<PathBuf> {
    rels.iter().map(PathBuf::from).collect()
}

/// Extracts what `reindex` asks for, as the caller would off the lock,
/// and puts it in.
fn run(index: &mut SymbolIndex, reindex: &Reindex) {
    let extracted = extract_all(
        index.canonical_root(),
        &reindex.extract,
        2,
        &AtomicBool::new(false),
    );
    index.update(extracted);
}

/// Where `name` is defined.
fn defined(index: &SymbolIndex, name: &str) -> Vec<PathBuf> {
    (index.definitions(name).into_iter())
        .map(|symbol| symbol.rel.to_path_buf())
        .collect()
}

#[test]
fn changed_files_are_reextracted() {
    let (_dir, root) = project();
    write(&root, "a.rs", "fn kept() {}\nfn deleted() {}\n");
    write(&root, "b.rs", "fn other() {}\n");
    let mut index = built(&root);

    write(&root, "a.rs", "fn kept() {}\nfn added() {}\n");
    let reindex = index.plan(&changes(&root, &["a.rs"]));
    assert_eq!(reindex.extract, paths(&["a.rs"]));
    run(&mut index, &reindex);

    assert_eq!(defined(&index, "added"), paths(&["a.rs"]));
    assert!(defined(&index, "deleted").is_empty());
    assert_eq!(defined(&index, "kept"), paths(&["a.rs"]));
    assert_eq!(defined(&index, "other"), paths(&["b.rs"]));
}

#[test]
fn removed_files_and_dirs_leave_the_index() {
    let (_dir, root) = project();
    write(&root, "a.rs", "fn a() {}\n");
    write(&root, "b.rs", "fn b() {}\n");
    write(&root, "src/m/x.rs", "fn x() {}\n");
    write(&root, "src/m/deep/y.rs", "struct Y;\n");
    let mut index = built(&root);

    fs::remove_file(root.join("b.rs")).unwrap();
    let reindex = index.plan(&changes(&root, &["b.rs"]));
    assert_eq!(reindex.extract, Vec::<PathBuf>::new());
    assert!(defined(&index, "b").is_empty());
    assert_eq!(index.file_state("b.rs"), None);

    fs::rename(root.join("src/m"), root.join("src/n")).unwrap();
    let reindex = index.plan(&changes(&root, &["src/m", "src/n"]));
    assert_eq!(reindex.extract, paths(&["src/n/deep/y.rs", "src/n/x.rs"]));
    // Gone at once, before the new paths are extracted.
    assert!(defined(&index, "x").is_empty());
    assert!(defined(&index, "Y").is_empty());
    run(&mut index, &reindex);

    assert_eq!(defined(&index, "x"), paths(&["src/n/x.rs"]));
    assert_eq!(defined(&index, "Y"), paths(&["src/n/deep/y.rs"]));
    assert_eq!(index.file_state("src/m/x.rs"), None);
    assert_eq!(index.file_state("src/m/deep/y.rs"), None);
    assert_eq!(index.progress(), (3, true));
}

#[test]
fn unchanged_and_ignored_files_are_not_reextracted() {
    let (_dir, root) = project();
    write(&root, ".gitignore", "ignored/\n");
    write(&root, "same.rs", "fn same() {}\n");
    write(&root, "locked.rs", "fn locked() {}\n");
    write(&root, "binary.rs", "fn binary() {}\n");
    write(&root, "notes.txt", "not code\n");
    let mut index = built(&root);

    // Rewritten with the same text and its old modification time: the same
    // length and time, so not read again.
    let state = index.file_state("same.rs").unwrap();
    write(&root, "same.rs", "fn same() {}\n");
    fs::File::options()
        .write(true)
        .open(root.join("same.rs"))
        .unwrap()
        .set_modified(state.1.unwrap())
        .unwrap();
    // A file whose language has no definitions isn't extracted either.
    write(&root, "notes.txt", "still not code\n");
    let reindex = index.plan(&changes(&root, &["same.rs", "notes.txt"]));
    assert_eq!(reindex.extract, Vec::<PathBuf>::new());

    // A new file in an ignored directory, reported by itself and by its
    // directory.
    write(&root, "ignored/new.rs", "fn hidden() {}\n");
    let reindex = index.plan(&changes(&root, &["ignored", "ignored/new.rs"]));
    assert_eq!(reindex.extract, Vec::<PathBuf>::new());

    // Files that can't be read now, or aren't text, lose their symbols
    // once extracted.
    write(&root, "locked.rs", "fn locked() {}\nfn more() {}\n");
    let locked = root.join("locked.rs");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // Unless permissions don't hold (as for root).
    let readable = fs::File::open(&locked).is_ok();
    write(&root, "binary.rs", b"fn binary() {}\n\0");
    let reindex = index.plan(&changes(&root, &["locked.rs", "binary.rs"]));
    assert_eq!(reindex.extract, paths(&["binary.rs", "locked.rs"]));
    assert_eq!(defined(&index, "binary"), paths(&["binary.rs"]));
    run(&mut index, &reindex);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(defined(&index, "binary").is_empty());
    assert_eq!(index.file_state("binary.rs"), None);
    if !readable {
        assert!(defined(&index, "locked").is_empty());
        assert_eq!(index.file_state("locked.rs"), None);
    }
    assert!(defined(&index, "hidden").is_empty());
    assert_eq!(defined(&index, "same"), paths(&["same.rs"]));
}

#[test]
fn rescan_reextracts_only_what_changed() {
    let (_dir, root) = project();
    for n in 0..50 {
        write(
            &root,
            &format!("f{n:02}.rs"),
            format!("fn f{n:02}() {{}}\n"),
        );
    }
    let mut index = built(&root);
    assert_eq!(index.progress(), (50, true));

    write(&root, "f10.rs", "fn f10() {}\nfn f10_more() {}\n");
    fs::remove_file(root.join("f20.rs")).unwrap();
    let reindex = index.plan(&[FsChange::Rescan]);
    assert_eq!(reindex.extract, paths(&["f10.rs"]));
    assert!(defined(&index, "f20").is_empty());
    run(&mut index, &reindex);

    assert_eq!(defined(&index, "f10_more"), paths(&["f10.rs"]));
    assert_eq!(defined(&index, "f11"), paths(&["f11.rs"]));
    assert_eq!(index.progress(), (49, true));
}

#[test]
fn changes_during_the_build_come_from_finish() {
    let (_dir, root) = project();
    write(&root, "a.rs", "fn a() {}\n");
    write(&root, "b.rs", "fn b() {}\n");
    write(&root, "c.rs", "fn c() {}\n");
    let mut index = SymbolIndex::new(&root);
    let mut planned = None;
    build(
        &root,
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| {
            index.extend(batch);
            if planned.is_none() {
                // After the first batch went in, a.rs changes and b.rs goes.
                write(&root, "a.rs", "fn a() {}\nfn a_late() {}\n");
                fs::remove_file(root.join("b.rs")).unwrap();
                planned = Some(index.plan(&changes(&root, &["a.rs", "b.rs"])));
            }
        },
    );
    assert_eq!(planned, Some(Reindex::default()));
    assert_eq!(defined(&index, "b"), paths(&["b.rs"]));

    let reindex = index.finish();
    assert_eq!(reindex.extract, paths(&["a.rs"]));
    assert!(defined(&index, "b").is_empty());
    run(&mut index, &reindex);

    assert_eq!(defined(&index, "a_late"), paths(&["a.rs"]));
    assert_eq!(defined(&index, "c"), paths(&["c.rs"]));
    assert_eq!(index.progress(), (2, true));
}

#[test]
fn a_late_result_does_not_bring_back_a_removed_file() {
    let (_dir, root) = project();
    write(&root, "a.rs", "fn a() {}\n");
    let mut index = built(&root);

    write(&root, "a.rs", "fn a() {}\nfn a2() {}\n");
    let reindex = index.plan(&changes(&root, &["a.rs"]));
    let extracted = extract_all(
        index.canonical_root(),
        &reindex.extract,
        1,
        &AtomicBool::new(false),
    );
    // Deleted, and planned, while the extraction was out.
    fs::remove_file(root.join("a.rs")).unwrap();
    assert_eq!(index.plan(&changes(&root, &["a.rs"])), Reindex::default());
    index.update(extracted);

    assert!(defined(&index, "a2").is_empty());
    assert_eq!(index.progress(), (0, true));
}

#[test]
fn a_file_added_and_removed_during_the_build_leaves_the_index() {
    let (_dir, root) = project();
    write(&root, "a.rs", "fn a() {}\n");
    let mut index = SymbolIndex::new(&root);
    index.extend(
        extract_all(
            index.canonical_root(),
            &paths(&["a.rs"]),
            1,
            &AtomicBool::new(false),
        )
        .files,
    );
    // A batch adds d/new.rs before the build's walk reaches it; the build
    // then hands it over too, and it's deleted before the build ends.
    write(&root, "d/new.rs", "fn ghost() {}\n");
    assert_eq!(
        index.plan(&changes(&root, &["d/new.rs"])),
        Reindex::default()
    );
    index.extend(
        extract_all(
            index.canonical_root(),
            &paths(&["d/new.rs"]),
            1,
            &AtomicBool::new(false),
        )
        .files,
    );
    assert_eq!(defined(&index, "ghost"), paths(&["d/new.rs"]));
    fs::remove_file(root.join("d/new.rs")).unwrap();
    assert_eq!(
        index.plan(&changes(&root, &["d/new.rs"])),
        Reindex::default()
    );

    assert_eq!(index.finish(), Reindex::default());
    assert!(defined(&index, "ghost").is_empty());
    assert_eq!(index.progress(), (1, true));
}

#[test]
fn a_root_swapped_for_a_symlink_is_not_followed() {
    let (_dir, tmp) = project();
    let root = tmp.join("project");
    write(&root, "a.rs", "fn a() {}\n");
    write(&tmp, "outside/a.rs", "fn external() {}\n");
    let mut index = built(&root);

    write(&root, "a.rs", "fn a() {}\nfn a2() {}\n");
    let reindex = index.plan(&changes(&root, &["a.rs"]));
    assert_eq!(reindex.extract, paths(&["a.rs"]));
    // Between planning and extracting, the root becomes a symlink to a
    // directory outside the project, with a file of the same name.
    fs::rename(&root, tmp.join("moved")).unwrap();
    std::os::unix::fs::symlink(tmp.join("outside"), &root).unwrap();
    run(&mut index, &reindex);

    assert!(defined(&index, "external").is_empty());
    assert!(defined(&index, "a2").is_empty());
}
