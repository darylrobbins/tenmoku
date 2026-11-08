//! Search filters under a relative root. This file holds one test only because
//! it sets the process working directory.

use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use fe_project::{PathFilter, SearchOptions, SearchQuery, search_filtered};

fn found(root: &str, filter: &PathFilter, threads: usize) -> Vec<String> {
    let options = SearchOptions {
        threads,
        ..SearchOptions::default()
    };
    let mut rels = Vec::new();
    search_filtered(
        root,
        &SearchQuery::literal("needle"),
        &options,
        filter,
        Arc::new(AtomicBool::new(false)),
        // Spelled with `/` on every platform, as the expectations are.
        |batch| {
            rels.extend(
                (batch.into_iter()).map(|file| file.rel.replace(std::path::MAIN_SEPARATOR, "/")),
            );
        },
    )
    .unwrap();
    rels.sort();
    rels
}

#[test]
fn globs_under_a_relative_root_match_paths_its_name_prefixes() {
    let dir = tempfile::tempdir().unwrap();
    for path in ["w/wa/x.txt", "w/y.txt"] {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "needle\n").unwrap();
    }
    std::env::set_current_dir(dir.path()).unwrap();

    let root = Path::new("w");
    for threads in [1, 0] {
        let filter = PathFilter::new(root, &["wa/"], &[]).unwrap();
        assert_eq!(found("w", &filter, threads), ["wa/x.txt"]);
        let filter = PathFilter::new(root, &[], &["wa/x.txt"]).unwrap();
        assert_eq!(found("w", &filter, threads), ["y.txt"]);
        let filter = PathFilter::new(root, &["*.txt"], &["y.txt"]).unwrap();
        assert_eq!(found("w", &filter, threads), ["wa/x.txt"]);
    }
}
