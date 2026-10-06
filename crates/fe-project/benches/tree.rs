//! The project tree on a generated 100,000-file project: 10 directories of
//! 10,000 empty files each. No budget is asserted; see PERF.md for numbers.
//!
//! Run with `cargo bench -p fe-project --bench tree`.

use std::{fs, hint::black_box};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fe_project::{FileTree, FsChange, TreeOptions, fixture};

fn benches(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fixture::generate(&root, 10, 10_000).unwrap();
    let options = TreeOptions::default();
    let big_dir = fixture::dir_name(0);

    let mut group = c.benchmark_group("tree");
    group.bench_function("open 100k-file root", |b| {
        b.iter(|| FileTree::open(black_box(&root), options).unwrap())
    });

    group.bench_function("expand 10k-file dir", |b| {
        b.iter_batched(
            || {
                let tree = FileTree::open(&root, options).unwrap();
                let id = tree.find(&big_dir).unwrap();
                (tree, id)
            },
            |(mut tree, id)| {
                tree.expand(id);
                tree
            },
            BatchSize::LargeInput,
        )
    });

    // A batch as a build or `git checkout` would send: 1,000 files touched in
    // a loaded, expanded directory. Each is stat'ed and found unchanged.
    let mut tree = FileTree::open(&root, options).unwrap();
    tree.expand(tree.find(&big_dir).unwrap());
    let batch: Vec<_> = (0..1_000)
        .map(|ix| FsChange::Path(root.join(&big_dir).join(fixture::file_name(ix * 10))))
        .collect();
    group.bench_function("apply 1k-path batch", |b| {
        b.iter(|| tree.apply(black_box(&batch)))
    });
    group.finish();
}

criterion_group!(tree, benches);
criterion_main!(tree);
