//! A root that is missing when its [`FinderIndex`] is made is resolved as far
//! as it exists, once: a `Rescan` walks it after it appears, while a root
//! swapped for a symlink later still isn't followed.

use super::*;

/// Every indexed path, in path order.
fn all(index: &mut FinderIndex) -> Vec<String> {
    index
        .query("", usize::MAX)
        .into_iter()
        .map(|found| slashed(found.path))
        .collect()
}

/// Makes `root` with `a.rs` in it and rescans, which finds it; then swaps
/// `root` for a symlink to a directory holding `b.rs` and rescans, which
/// finds nothing.
fn create_then_swap(index: &mut FinderIndex, root: &Path) {
    make(root, &["a.rs"]);
    index.apply(&[FsChange::Rescan]);
    assert_eq!(all(index), ["a.rs"]);

    let elsewhere = root.with_file_name("elsewhere");
    make(&elsewhere, &["b.rs"]);
    swap_for_link(root, &elsewhere);
    index.apply(&[FsChange::Rescan]);
    assert!(all(index).is_empty());
}

#[test]
fn rescan_finds_a_relative_root_made_after_the_index() {
    let (_dir, base) = temp();
    let root = base.join("missing-project");
    let Some(rel) = relative(&root) else {
        eprintln!("skipped: the temporary folder is on another drive");
        return;
    };
    let mut index = FinderIndex::new(rel);
    index.finish();
    assert!(all(&mut index).is_empty());

    create_then_swap(&mut index, &root);
    assert_eq!(index.root_path(), root);
}

#[test]
fn rescan_finds_a_missing_root_under_a_symlinked_ancestor() {
    let (_dir, base) = temp();
    make(&base, &["real/"]);
    dir_link(&base.join("real"), &base.join("link"));
    let mut index = FinderIndex::new(base.join("link/missing-project"));
    index.finish();
    assert!(all(&mut index).is_empty());

    let root = base.join("real/missing-project");
    create_then_swap(&mut index, &root);
    assert_eq!(index.root_path(), root);
}
