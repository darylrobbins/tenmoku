//! The Cmd-P finder on 100,000 paths: walking a generated project of 100
//! directories of 1,000 empty files each, and queries of 1, 3, 8 and 20 chars
//! (limit 50) over 100,000 in-memory paths shaped like
//! `crates/<name>/src/<dir>/<file>.rs`. No budget is asserted; the plan's is
//! first results in 50 ms. See PERF.md for numbers.
//!
//! Run with `cargo bench -p fe-project --bench finder`.

use std::{
    fs,
    hint::black_box,
    sync::{Arc, atomic::AtomicBool},
};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fe_project::{FinderIndex, fixture};

const WORDS: [&str; 20] = [
    "buffer", "cursor", "display", "editor", "finder", "gutter", "history", "input", "layout",
    "motion", "parser", "query", "render", "search", "syntax", "theme", "undo", "view", "window",
    "worker",
];

/// 100,000 distinct paths: 50 crates of 20 directories of 100 files.
fn paths() -> Vec<String> {
    let mut paths = Vec::with_capacity(100_000);
    for krate in 0..50 {
        let krate = format!("fe-{}-{}", WORDS[krate % 20], krate / 20);
        for dir in 0..20 {
            for file in 0..100 {
                paths.push(format!(
                    "crates/{krate}/src/{}/{}_{}_{file}.rs",
                    WORDS[dir],
                    WORDS[(file + dir) % 20],
                    WORDS[file % 7],
                ));
            }
        }
    }
    paths
}

fn benches(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fixture::generate(&root, 100, 1_000).unwrap();

    let mut group = c.benchmark_group("finder");
    group.sample_size(20);
    group.bench_function("walk 100k files", |b| {
        b.iter_batched(
            || FinderIndex::new(&root),
            |mut index| {
                FinderIndex::walk(&root, Arc::new(AtomicBool::new(false)), |batch| {
                    index.extend(batch)
                });
                index.finish();
                index
            },
            BatchSize::LargeInput,
        )
    });

    let mut index = FinderIndex::from_paths(paths());
    for needle in ["e", "edv", "editview", "crates/editor/viewrs"] {
        assert_eq!(index.query(needle, 50).len(), 50, "{needle} fills the page");
        group.bench_function(format!("query {} chars", needle.len()), |b| {
            b.iter(|| index.query(black_box(needle), 50))
        });
    }
    group.finish();
}

criterion_group!(finder, benches);
criterion_main!(finder);
