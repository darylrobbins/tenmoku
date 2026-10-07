//! Navigation over a generated corpus of 10,000 Rust-like files (100
//! directories of 100) of about 4 KB each, about 40 MB, plus a
//! `.gitignore`d directory: the index build (default threads and one),
//! go-to-def with a complete index and through the ripgrep fallback,
//! references to the first batch and to completion, and workspace symbols
//! for 1-, 3- and 6-char queries. Nothing is asserted; the plan's target
//! is go-to-def in 100 ms before the language server is ready. See PERF.md
//! for numbers.
//!
//! Every file defines a struct, an enum, an impl, `handle` (so the common
//! name is defined 10,000 times), `shared_N` (one of 200 names, each
//! defined in 50 files) and some functions of its own, and calls `shared_N`
//! (a same-file hit), the next `shared` name (50 definitions elsewhere) and
//! a function of another file. Every 2,000th file defines `rare_needle_fn`
//! and five files call it.
//!
//! Run with `cargo bench -p fe-nav --bench nav`.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fe_buffer::Text;
use fe_nav::{
    BuildOptions, NavRequest, NavigationProvider, ReferencesSummary, SearchNavigation, SymbolIndex,
    build,
};

const FILES: usize = 10_000;
const SHARED: usize = 200;

const WORDS: [&str; 20] = [
    "buffer", "cursor", "display", "editor", "finder", "gutter", "history", "input", "layout",
    "motion", "parser", "query", "render", "search", "syntax", "theme", "undo", "view", "window",
    "worker",
];

fn rel(ix: usize) -> String {
    format!(
        "{}_{:03}/{}_{:03}.rs",
        WORDS[ix / 100 % 20],
        ix / 100,
        WORDS[ix % 20],
        ix % 100
    )
}

fn module(ix: usize) -> String {
    format!(
        "{}_{:03}::{}_{:03}",
        WORDS[ix / 100 % 20],
        ix / 100,
        WORDS[ix % 20],
        ix % 100
    )
}

/// One file of about 4 KB.
fn file(ix: usize) -> String {
    let shared = ix % SHARED;
    let next = (ix % SHARED + 1) % SHARED;
    let other = (ix * 37 + 11) % FILES;
    let word = WORDS[ix % 20];
    let mut text = String::with_capacity(4_400);
    writeln!(text, "//! The {word} module, number {ix}.\n").unwrap();
    writeln!(
        text,
        "pub struct Item{ix} {{\n    pub {word}_len: usize,\n    pub {word}_id: u32,\n}}\n"
    )
    .unwrap();
    writeln!(
        text,
        "pub enum Kind{ix} {{\n    First,\n    Second(u32),\n}}\n"
    )
    .unwrap();
    writeln!(
        text,
        "impl Item{ix} {{\n    pub fn new() -> Self {{\n        Self {{ {word}_len: 0, {word}_id: {ix} }}\n    }}\n}}\n"
    )
    .unwrap();
    writeln!(
        text,
        "pub fn handle(input: usize) -> usize {{\n    input + {ix}\n}}\n"
    )
    .unwrap();
    writeln!(
        text,
        "pub fn shared_{shared:03}(input: usize) -> usize {{\n    input * {ix}\n}}\n"
    )
    .unwrap();
    writeln!(
        text,
        "pub fn entry_{ix}(input: usize) -> usize {{\n    let first = shared_{shared:03}(input);\n    let second = shared_{next:03}(first);\n    crate::{}::local_{other}_0(second) + handle(first)\n}}\n",
        module(other)
    )
    .unwrap();
    if ix % 2_000 == 5 {
        writeln!(
            text,
            "pub fn use_rare(input: usize) -> usize {{\n    rare_needle_fn(input)\n}}\n"
        )
        .unwrap();
    }
    if ix == 4_242 {
        writeln!(
            text,
            "pub fn rare_needle_fn(input: usize) -> usize {{\n    input\n}}\n"
        )
        .unwrap();
    }
    let mut n = 0;
    while text.len() < 4_000 {
        let w = WORDS[(ix + n) % 20];
        writeln!(
            text,
            "/// Update the {w} before the next frame.\npub fn local_{ix}_{n}(input: usize) -> usize {{\n    let {w} = input.{w}_len() + {n};\n    {w} // {w}\n}}\n"
        )
        .unwrap();
        n += 1;
    }
    text
}

fn generate(root: &Path) {
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    let target = root.join("target");
    fs::create_dir_all(&target).unwrap();
    for ix in 0..100 {
        fs::write(target.join(format!("ignored_{ix}.rs")), file(ix)).unwrap();
    }
    for ix in 0..FILES {
        let path = root.join(rel(ix));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, file(ix)).unwrap();
    }
}

fn no_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn build_index(root: &Path, threads: usize) -> SymbolIndex {
    let mut index = SymbolIndex::new(root);
    let summary = build(root, &BuildOptions { threads }, no_cancel(), |batch| {
        index.extend(batch)
    });
    index.finish();
    assert_eq!(summary.files_indexed, FILES);
    index
}

/// The process's peak resident set, in bytes.
fn peak_rss() -> u64 {
    // SAFETY: `getrusage` fills in the struct it's given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    // Bytes on macOS, kilobytes elsewhere.
    let max = usage.ru_maxrss as u64;
    if cfg!(target_os = "macos") {
        max
    } else {
        max * 1024
    }
}

/// A request from file `ix`, with the caret on the first `at` in its text.
fn request(root: &Path, ix: usize, at: &str) -> NavRequest {
    let rel = PathBuf::from(rel(ix));
    let text = fs::read_to_string(root.join(&rel)).unwrap();
    let offset = text.find(at).unwrap_or_else(|| panic!("{at} in file {ix}"));
    NavRequest {
        root: root.to_path_buf(),
        rel,
        language: Some("rust"),
        text: Text::new(&text),
        offset,
    }
}

fn definitions(nav: &SearchNavigation, req: &NavRequest) -> fe_nav::Definitions {
    nav.definitions(req, &AtomicBool::new(false)).unwrap()
}

fn references(
    nav: &SearchNavigation,
    req: &NavRequest,
    first_only: bool,
) -> (Duration, usize, ReferencesSummary) {
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let mut first = None;
    let mut files = 0;
    let summary = nav.references(req, &cancel, &mut |batch| {
        files += batch.len();
        first.get_or_insert_with(|| start.elapsed());
        if first_only {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    let elapsed = start.elapsed();
    let summary = summary.unwrap();
    (
        if first_only {
            first.expect("a batch")
        } else {
            elapsed
        },
        files,
        summary,
    )
}

fn benches(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    generate(&root);
    let bytes: u64 = (0..FILES).map(|ix| file(ix).len() as u64).sum();
    println!("corpus: {FILES} files, {:.1} MB", bytes as f64 / 1e6);

    let before = peak_rss();
    let index = build_index(&root, 0);
    let after = peak_rss();
    let stats = index.stats();
    println!(
        "index: {} files, {} symbols, {} distinct names, approx_bytes {} ({:.1} MB); peak RSS {:.0} MB -> {:.0} MB (+{:.0} MB)",
        stats.files,
        stats.symbols,
        stats.distinct_names,
        stats.approx_bytes,
        stats.approx_bytes as f64 / 1e6,
        before as f64 / 1e6,
        after as f64 / 1e6,
        (after - before) as f64 / 1e6,
    );
    assert_eq!(index.definitions("rare_needle_fn").len(), 1);
    assert_eq!(index.definitions("shared_007").len(), FILES / SHARED);
    assert_eq!(index.definitions("handle").len(), FILES);

    let mut group = c.benchmark_group("nav build 10k files");
    group.sample_size(10);
    group.throughput(Throughput::Elements(FILES as u64));
    group.bench_function("default threads", |b| b.iter(|| build_index(&root, 0)));
    group.bench_function("threads: 1", |b| b.iter(|| build_index(&root, 1)));
    group.finish();

    // Requests. File 5 calls the rare name; file 4242 defines it.
    let rare = request(&root, 5, "rare_needle_fn");
    let many = request(&root, 1, "shared_002(first)");
    let same = request(&root, 1, "shared_001(input)");
    let common = request(&root, 1, "handle(first)");
    assert_ne!(rare.rel, PathBuf::from(rel(4_242)));

    let complete = SearchNavigation::new(&root, Arc::new(RwLock::new(index)));
    let mut group = c.benchmark_group("go-to-def, index complete");
    for (name, req) in [
        ("rare name", &rare),
        ("name in 50 files", &many),
        ("same-file hit", &same),
    ] {
        let answer = definitions(&complete, req);
        println!(
            "{name}: {} items, unambiguous {}, partial {}",
            answer.items.len(),
            answer.unambiguous,
            answer.partial
        );
        assert!(!answer.items.is_empty() && !answer.partial);
        group.bench_function(name, |b| b.iter(|| definitions(&complete, req)));
    }
    group.finish();

    // An empty index that never finishes: every request takes the fallback.
    let fallback = SearchNavigation::new(&root, Arc::new(RwLock::new(SymbolIndex::new(&root))));
    let mut group = c.benchmark_group("go-to-def, ripgrep fallback");
    group.sample_size(20);
    for (name, req) in [("rare name", &rare), ("common name", &common)] {
        let answer = definitions(&fallback, req);
        println!(
            "fallback {name}: {} items, partial {}",
            answer.items.len(),
            answer.partial
        );
        group.bench_function(name, |b| b.iter(|| definitions(&fallback, req)));
    }
    group.finish();
    println!("fallback stats: {:?}", fallback.stats());

    let mut group = c.benchmark_group("references");
    group.sample_size(20);
    for (name, req) in [("rare name", &rare), ("common name", &common)] {
        let (_, files, summary) = references(&complete, req, false);
        println!(
            "references {name}: {files} files with references; searched {}, verified {}, dropped {}, truncated {}, cancelled {}",
            summary.files_searched,
            summary.files_verified,
            summary.dropped,
            summary.truncated,
            summary.cancelled
        );
        group.bench_function(format!("{name}, first batch"), |b| {
            b.iter_custom(|iters| (0..iters).map(|_| references(&complete, req, true).0).sum())
        });
        group.bench_function(format!("{name}, complete"), |b| {
            b.iter_custom(|iters| {
                (0..iters)
                    .map(|_| references(&complete, req, false).0)
                    .sum()
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("workspace symbols");
    for (name, query) in [("1 char", "s"), ("3 chars", "han"), ("6 chars", "shared")] {
        let found = complete
            .workspace_symbols(query, 50, &AtomicBool::new(false))
            .unwrap();
        println!(
            "symbols {name}: {} results, first {:?}",
            found.len(),
            found.first().map(|s| &s.name)
        );
        group.bench_function(name, |b| {
            b.iter(|| {
                complete
                    .workspace_symbols(query, 50, &AtomicBool::new(false))
                    .unwrap()
            })
        });
    }
    group.finish();
}

criterion_group!(nav_benches, benches);
criterion_main!(nav_benches);
