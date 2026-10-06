//! Project search over a generated corpus of 10,000 files (100 directories
//! of 100) of about 4 KB of Rust-like lines each, about 40 MB, plus a
//! `.gitignore`d directory: a rare literal, a common literal that hits the
//! default cap, a regex, a case-insensitive whole-word literal, and the time
//! to the first batch. No budget is asserted; the provisional target is a
//! first batch in 50 ms. See PERF.md for numbers.
//!
//! Run with `cargo bench -p fe-project --bench search`.

use std::{
    fmt::Write as _,
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use fe_project::{SearchOptions, SearchQuery, SearchSummary, search};

const WORDS: [&str; 20] = [
    "buffer", "cursor", "display", "editor", "finder", "gutter", "history", "input", "layout",
    "motion", "parser", "query", "render", "search", "syntax", "theme", "undo", "view", "window",
    "worker",
];

/// One file of about 4 KB. Every 2,000th file holds the rare literal.
fn file(ix: usize) -> String {
    let mut text = String::with_capacity(4_200);
    let mut line = 0;
    while text.len() < 4_000 {
        let word = WORDS[(ix + line) % 20];
        let other = WORDS[(ix * 7 + line * 3) % 20];
        match line % 4 {
            0 => writeln!(
                text,
                "pub fn {word}_{line}(input: &{}) -> usize {{",
                capital(other)
            ),
            1 => writeln!(text, "    let {word} = input.{other}_len() + {ix};"),
            2 => writeln!(
                text,
                "    // Update the {other} {word} before the next frame."
            ),
            _ => writeln!(text, "    {word}\n}}\n"),
        }
        .unwrap();
        if ix.is_multiple_of(2_000) && line == 10 {
            text.push_str("    let rare_needle_token = 1;\n");
        }
        line += 1;
    }
    text
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + chars.as_str()
    })
}

fn generate(root: &Path) {
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    let target = root.join("target");
    fs::create_dir_all(&target).unwrap();
    for ix in 0..100 {
        fs::write(target.join(format!("ignored_{ix}.rs")), file(ix)).unwrap();
    }
    for dir in 0..100 {
        let dir_path = root.join(format!("{}_{dir:03}", WORDS[dir % 20]));
        fs::create_dir_all(&dir_path).unwrap();
        for file_ix in 0..100 {
            let ix = dir * 100 + file_ix;
            fs::write(
                dir_path.join(format!("{}_{file_ix:03}.rs", WORDS[ix % 20])),
                file(ix),
            )
            .unwrap();
        }
    }
}

fn run(root: &Path, query: &SearchQuery) -> SearchSummary {
    search(
        root,
        query,
        &SearchOptions::default(),
        Arc::new(AtomicBool::new(false)),
        |batch| {
            std::hint::black_box(batch);
        },
    )
    .unwrap()
}

fn benches(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    generate(&root);

    let rare = SearchQuery::literal("rare_needle_token");
    let common = SearchQuery::literal("let");
    let regex = SearchQuery {
        regex: true,
        ..SearchQuery::literal(r"fn \w+_\d+\(")
    };
    let word = SearchQuery {
        whole_word: true,
        ..SearchQuery::literal("Buffer")
    };
    let rare_summary = run(&root, &rare);
    assert_eq!(
        rare_summary.files_searched, 10_001,
        "the files and .gitignore, not the ignored directory"
    );
    assert_eq!(rare_summary.matches, 5);
    assert!(
        run(&root, &common).truncated,
        "the common literal hits the cap"
    );

    let mut group = c.benchmark_group("search 10k files");
    group.sample_size(20);
    for (name, query) in [
        ("rare literal", &rare),
        ("common literal, capped", &common),
        ("regex", &regex),
        ("whole word, case-insensitive", &word),
    ] {
        let summary = run(&root, query);
        println!(
            "{name}: {} matches in {} files, {} files searched{}",
            summary.matches,
            summary.files_matched,
            summary.files_searched,
            if summary.truncated { ", capped" } else { "" },
        );
        group.bench_function(name, |b| b.iter(|| run(&root, query)));
    }
    group.bench_function("first batch", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let cancel = Arc::new(AtomicBool::new(false));
                let start = Instant::now();
                let mut first = None;
                search(
                    &root,
                    &common,
                    &SearchOptions::default(),
                    cancel.clone(),
                    |_| {
                        first.get_or_insert_with(|| start.elapsed());
                        cancel.store(true, Ordering::Relaxed);
                    },
                )
                .unwrap();
                total += first.expect("a batch arrives");
            }
            total
        })
    });
    group.finish();
}

criterion_group!(search_benches, benches);
criterion_main!(search_benches);
