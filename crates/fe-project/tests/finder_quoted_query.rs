use fe_project::FinderIndex;

fn index() -> FinderIndex {
    FinderIndex::from_paths([
        "src/main.rs",
        "src/m_a_i_n.rs",
        "docs/Domain.md",
        "main/m_a_i_n.txt",
        "src/lib.rs",
    ])
}

#[test]
fn quoted_words_match_contiguously_as_vs_code_does() {
    let mut index = index();
    let mut paths = |text: &str| -> Vec<String> {
        let mut found: Vec<String> = index.query(text, 10).into_iter().map(|m| m.path).collect();
        found.sort();
        found
    };
    let main = ["docs/Domain.md", "main/m_a_i_n.txt", "src/main.rs"];
    assert_eq!(paths("\"main\""), main);
    assert_eq!(paths("\"MAIN\""), main);
    assert_eq!(paths("\"src/main\""), ["src/main.rs"]);
    assert_eq!(paths("\"main\" \"rs\""), ["src/main.rs"]);
    assert_eq!(paths("\"main rs\""), paths("main rs"));
    assert_eq!(paths("\"main"), paths("main"));
    assert_eq!(paths("\"main, rs"), paths("main, rs"));
    assert_eq!(paths("\"\"").len(), 5);
    assert_eq!(paths("\"").len(), 5);

    let found = index.query("\"main\"", 10);
    let indices = |path: &str| {
        found
            .iter()
            .find(|m| m.path == path)
            .unwrap()
            .indices
            .clone()
    };
    assert_eq!(indices("docs/Domain.md"), [7, 8, 9, 10]);
    assert_eq!(indices("main/m_a_i_n.txt"), [0, 1, 2, 3]);
    assert_eq!(indices("src/main.rs"), [4, 5, 6, 7]);
}
