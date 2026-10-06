//! Results that saw a directory gone, filled after the root was seen cut off.

use super::*;
use crate::listing::Lost;

/// A tree of `up/proj` with `p` listed and `p/c` not, and a listing of `c`
/// run while `c` was away for a moment.
fn away_for_a_moment() -> (TempDir, PathBuf, FileTree, NodeId, Listed) {
    let (dir, base) = temp();
    let root = base.join("up/proj");
    make(&root, &["p/c/old.txt", "w/"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "p"));
    let c = id(&tree, "p/c");
    let listing = tree.listing(c).unwrap();
    let away = root.join("p/c");
    fs::rename(&away, aside(&away)).unwrap();
    let gone = listing.run();
    swap_back_dir(&away);
    assert_eq!(gone.lost, Some(Lost::Itself));
    (dir, base, tree, c, gone)
}

/// `c`, back, is still in the tree and is listed again.
fn kept(tree: &mut FileTree, c: NodeId) {
    assert_eq!(tree.find("p/c"), Some(c));
    let listing = tree.listing(c).expect("c owes a listing");
    tree.fill(c, listing.run());
    assert!(tree.find("p/c/old.txt").is_some());
}

#[test]
fn a_gone_result_older_than_a_cut_seen_by_a_listing_keeps_the_directory() {
    // The root seen cut off by a listing of another directory: changes made
    // meanwhile, `c`'s return among them, may have gone unreported.
    let (_dir, base, mut tree, c, gone) = away_for_a_moment();
    let up = base.join("up");
    fs::rename(&up, aside(&up)).unwrap();
    let w = id(&tree, "w");
    let cut = tree.listing(w).unwrap().run();
    assert_eq!(cut.lost, Some(Lost::Root));
    tree.fill(w, cut);
    swap_back_dir(&up);
    tree.fill(c, gone);
    kept(&mut tree, c);
}

#[test]
fn a_gone_result_older_than_a_rescan_cut_short_keeps_the_directory() {
    // A rescan that couldn't reach the root, so marked nothing below it.
    let (_dir, base, mut tree, c, gone) = away_for_a_moment();
    let up = base.join("up");
    fs::rename(&up, aside(&up)).unwrap();
    tree.apply(&[FsChange::Rescan]);
    swap_back_dir(&up);
    tree.fill(c, gone);
    kept(&mut tree, c);
}
