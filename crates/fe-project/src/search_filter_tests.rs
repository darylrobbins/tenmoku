use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use super::*;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for path in ["src/a.rs", "src/b.txt", "docs/c.rs", "target/d.rs"] {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "needle\n").unwrap();
    }
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    (dir, root)
}

fn found(root: &Path, filter: &PathFilter, threads: usize) -> (Vec<String>, SearchSummary) {
    let options = SearchOptions {
        threads,
        ..SearchOptions::default()
    };
    let mut rels = Vec::new();
    let summary = search_filtered(
        root,
        &SearchQuery::literal("needle"),
        &options,
        filter,
        Arc::new(AtomicBool::new(false)),
        |batch| rels.extend(batch.into_iter().map(|file| file.rel)),
    )
    .unwrap();
    rels.sort();
    (rels, summary)
}

#[test]
fn include_narrows() {
    let (_dir, root) = fixture();
    for threads in [1, 0] {
        let filter = PathFilter::new(&root, &["*.rs"], &[]).unwrap();
        assert_eq!(found(&root, &filter, threads).0, ["docs/c.rs", "src/a.rs"]);
        let filter = PathFilter::new(&root, &["src/**"], &[]).unwrap();
        assert_eq!(found(&root, &filter, threads).0, ["src/a.rs", "src/b.txt"]);
    }
}

#[test]
fn exclude_prunes_and_wins() {
    let (_dir, root) = fixture();
    for threads in [1, 0] {
        let filter = PathFilter::new(&root, &[], &["docs/"]).unwrap();
        let (rels, summary) = found(&root, &filter, threads);
        assert_eq!(rels, ["src/a.rs", "src/b.txt"]);
        // .gitignore itself holds no needle, but is read: a, b and it.
        assert_eq!(summary.files_searched, 3);

        let filter = PathFilter::new(&root, &["*.rs"], &["src/a.rs"]).unwrap();
        assert_eq!(found(&root, &filter, threads).0, ["docs/c.rs"]);
    }
}

#[test]
fn errors_and_defaults() {
    let (_dir, root) = fixture();
    let err = PathFilter::new(&root, &["a["], &[]).unwrap_err();
    assert_eq!(err.glob, "a[");
    assert!(err.to_string().contains("a["));
    assert!(PathFilter::new(&root, &[], &["a["]).is_err());

    for threads in [1, 0] {
        let options = SearchOptions {
            threads,
            ..SearchOptions::default()
        };
        let mut plain = Vec::new();
        search(
            &root,
            &SearchQuery::literal("needle"),
            &options,
            Arc::default(),
            |batch| plain.extend(batch.into_iter().map(|file| file.rel)),
        )
        .unwrap();
        plain.sort();
        let (rels, _) = found(&root, &PathFilter::default(), threads);
        assert_eq!(rels, plain);
        assert_eq!(rels, ["docs/c.rs", "src/a.rs", "src/b.txt"]);
    }
}
