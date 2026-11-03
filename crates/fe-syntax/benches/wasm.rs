//! Kotlin as a built-in grammar against the same grammar as WebAssembly.
//!
//! For each of the two, on the golden sample (`tests/golden/kotlin.kt`) and
//! on a corpus of it repeated to about 5,000 lines (generated here, not
//! committed): a full parse, a full highlight, and one incremental reparse
//! after a single-character edit in the middle of the corpus. The
//! WebAssembly grammar's load cost (wasmtime compile and instantiate, plus
//! its queries) is timed once more, apart.
//!
//! The module is `target/grammar-wasm/tree-sitter-kotlin.wasm`, which
//! `scripts/build-grammar-wasm.sh kotlin` builds, loaded as a user grammar
//! is, with the queries of `extensions/grammars/kotlin/`.
//!
//! Run with `cargo bench -p fe-syntax --bench wasm`. The settings are
//! criterion's quick ones: a small sample, a short measurement.

use std::{
    fs,
    hint::black_box,
    path::{Path, PathBuf},
    process::exit,
    time::{Duration, Instant},
};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use fe_buffer::{Buffer, Origin};
use fe_syntax::{
    GrammarSpec, Language, Syntax, language_by_name, register_language, unregister_language,
};

const SCRIPT: &str = "scripts/build-grammar-wasm.sh";
const LARGE_LINES: usize = 5_000;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn spec() -> GrammarSpec {
    let wasm = repository().join("target/grammar-wasm/tree-sitter-kotlin.wasm");
    if !wasm.is_file() {
        eprintln!(
            "{} doesn't exist: build it with {SCRIPT} kotlin",
            wasm.display()
        );
        exit(1);
    }
    let dir = repository().join("extensions/grammars/kotlin");
    GrammarSpec {
        language: "kotlin_wasm".to_owned(),
        wasm,
        highlights: Some(dir.join("highlights.scm")),
        injections: Some(dir.join("injections.scm")),
        earlier_patterns_win: false,
        ..GrammarSpec::default()
    }
}

/// The sample repeated whole to `LARGE_LINES` lines. Later copies leave out
/// the package and imports, which may only open a file.
fn large(sample: &str) -> String {
    let body: String = sample
        .lines()
        .filter(|line| !line.starts_with("package ") && !line.starts_with("import "))
        .map(|line| format!("{line}\n"))
        .collect();
    let mut text = sample.to_owned();
    while text.matches('\n').count() + body.matches('\n').count() <= LARGE_LINES {
        text.push_str(&body);
    }
    text
}

/// The offset of a letter in the middle of the text, where a character goes.
fn middle(text: &str) -> usize {
    let half = text.len() / 2;
    half + text[half..]
        .find(|c: char| c.is_ascii_lowercase())
        .expect("a letter follows the middle")
}

fn benches(c: &mut Criterion) {
    let native = language_by_name("kotlin").expect("kotlin is compiled in");
    let spec = spec();

    // Compile and instantiate again each time: unregistering drops the cached
    // grammar, so the next registration loads the module.
    let mut group = c.benchmark_group("wasm load");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));
    group.warm_up_time(Duration::from_secs(1));
    group.bench_function("kotlin", |b| {
        b.iter(|| {
            unregister_language("kotlin_wasm");
            black_box(register_language(&spec).expect("the module loads"))
        })
    });
    group.finish();
    unregister_language("kotlin_wasm");
    let wasm: &'static Language = register_language(&spec).expect("the module loads");

    let sample = fs::read_to_string(repository().join("crates/fe-syntax/tests/golden/kotlin.kt"))
        .expect("the golden sample");
    let corpora = [
        ("golden sample", sample.clone()),
        ("5k lines", large(&sample)),
    ];
    let mut group = c.benchmark_group("kotlin");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(2));
    group.warm_up_time(Duration::from_secs(1));

    for (size, text) in &corpora {
        eprintln!(
            "kotlin {size}: {} bytes, {} lines",
            text.len(),
            text.matches('\n').count()
        );
        for (engine, language) in [("native", native), ("wasm", wasm)] {
            let id = |what: &str| BenchmarkId::new(format!("{what} {engine}"), size);
            let mut buffer = Buffer::new(text);
            let mut syntax = Syntax::new(language, &buffer.snapshot());

            group.bench_function(id("parse"), |b| b.iter(|| syntax.parse_job().run()));
            syntax.finish(syntax.parse_job().run());
            let tree = syntax.tree().expect("the text parses");
            assert!(
                !tree.root_node().has_error(),
                "kotlin {size} parses without errors"
            );

            // Highlights are cached by range, so each run gets a parsed
            // `Syntax` of its own, made untimed.
            group.bench_function(id("highlight"), |b| {
                b.iter_batched_ref(
                    || {
                        let mut fresh = Syntax::new(language, &buffer.snapshot());
                        fresh.finish(fresh.parse_job().run());
                        fresh
                    },
                    |fresh| black_box(fresh.highlights(0..text.len())),
                    BatchSize::PerIteration,
                )
            });

            if *size == "5k lines" {
                // The edit is undone, untimed, so errors don't pile up.
                let offset = middle(text);
                group.bench_function(id("reparse one character"), |b| {
                    let reparse = |buffer: &Buffer, syntax: &mut Syntax| {
                        syntax.edit(&buffer.snapshot());
                        let started = Instant::now();
                        assert!(syntax.parse_within(Duration::from_secs(60)));
                        started.elapsed()
                    };
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            buffer.edit([(offset..offset, "x")], Origin::User);
                            total += reparse(&buffer, &mut syntax);
                            buffer.edit([(offset..offset + 1, "")], Origin::User);
                            reparse(&buffer, &mut syntax);
                        }
                        total
                    })
                });
            }
        }
    }
    group.finish();
}

criterion_group!(wasm, benches);
criterion_main!(wasm);
