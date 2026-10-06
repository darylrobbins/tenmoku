use super::*;
use crate::fixture;

/// The only test that generates the 100k-file fixture: nextest runs each test
/// in its own process, so sharing it between tests would mean making it twice.
#[test]
fn a_100k_file_tree_loads_lazily() {
    let (_dir, root) = temp();
    fixture::generate(&root, 100, 1_000).unwrap();

    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    let root_entries = tree.children(tree.root()).unwrap().len();
    // The directories and `.gitignore`; the ignored directory is hidden.
    assert_eq!(root_entries, 101);
    assert_eq!(tree.stats().loaded_entries, root_entries);
    assert!(tree.find(fixture::IGNORED_DIR).is_none());

    tree.expand(id(&tree, &fixture::dir_name(0)));
    let stats = tree.stats();
    assert_eq!(stats.listings, 2);
    assert_eq!(stats.loaded_entries, root_entries + 1_000);
    assert_eq!(tree.row_count(), root_entries + 1_000);

    let rows: Vec<_> = tree.rows(0..50).map(|row| describe(&row)).collect();
    assert_eq!(rows.len(), 50);
    assert_eq!(rows[0], "v dir_0000");
    assert_eq!(rows[1], "  file_00000.txt");
    assert_eq!(rows[49], "  file_00048.txt");
}
