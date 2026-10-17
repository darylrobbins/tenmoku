use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
};

use fe_project::FinderIndex;

#[test]
fn space_separated_words_each_match_the_path() {
    let dir = tempfile::tempdir().unwrap();
    for path in ["src/editor/main.rs", "src/lib.rs", "docs/main.md"] {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "").unwrap();
    }
    let mut index = FinderIndex::new(dir.path());
    FinderIndex::walk(dir.path(), Arc::new(AtomicBool::new(false)), |batch| {
        index.extend(batch)
    });
    index.finish();

    let mut paths =
        |text: &str| -> Vec<String> { index.query(text, 10).into_iter().map(|m| m.path).collect() };
    for text in ["main rs", "rs main", "editor main", "src main rs"] {
        assert_eq!(paths(text), ["src/editor/main.rs"], "{text:?}");
    }
    assert_eq!(paths("main md"), ["docs/main.md"]);
    assert!(paths("main zzz").is_empty());
    let main = paths("main");
    assert_eq!(main.len(), 2);
    for text in ["main ", " main", "main  "] {
        assert_eq!(paths(text), main, "{text:?}");
    }
    assert_eq!(paths("   "), paths(""));
    assert_eq!(paths("").len(), 3);

    for text in ["main rs", "rs main", "editor main", "src main rs"] {
        for m in index.query(text, 10) {
            let chars: Vec<char> = m.path.chars().collect();
            assert!(m.indices.windows(2).all(|w| w[0] < w[1]), "{text:?}");
            let picked: String = m.indices.iter().map(|&i| chars[i]).collect();
            for word in text.split(' ') {
                let mut rest = picked.chars();
                assert!(
                    word.chars()
                        .all(|w| rest.any(|c| c.eq_ignore_ascii_case(&w))),
                    "{text:?}: {word:?} not spelled by {picked:?}"
                );
            }
        }
    }
}
