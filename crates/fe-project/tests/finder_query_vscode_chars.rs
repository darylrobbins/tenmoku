use fe_project::FinderIndex;

#[test]
fn backslashes_wildcards_and_ellipses_read_as_vs_code_reads_them() {
    let mut index = FinderIndex::from_paths(["src/editor/main.rs", "src/lib.rs", "docs/main.md"]);
    let mut paths =
        |text: &str| -> Vec<String> { index.query(text, 10).into_iter().map(|m| m.path).collect() };
    assert_eq!(paths(r"editor\main"), ["src/editor/main.rs"]);
    assert_eq!(paths("*.md"), ["docs/main.md"]);
    assert_eq!(paths("main*rs"), ["src/editor/main.rs"]);
    assert_eq!(paths("src…lib"), ["src/lib.rs"]);
    assert_eq!(paths("*").len(), 3);
    assert_eq!(paths("* …"), paths(""));

    let found = index.query(r"src\lib", 10);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "src/lib.rs");
    assert_eq!(found[0].indices, [0, 1, 2, 3, 4, 5, 6]);
}
