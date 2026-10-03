//! Parse, reparse and highlight costs, for Rust, JSON and Markdown.
//!
//! Sizes:
//! - 10k lines: a typical large source file;
//! - just under [`MAX_PARSE_BYTES`]: the largest file that is highlighted;
//! - 1M lines: full parse only. Every 1M-line corpus here is over the parse
//!   limit, so the editor shows it unhighlighted; the number says what
//!   highlighting it would cost.
//!
//! A reparse follows one keystroke inside a word somewhere random. A
//! highlight is 60 rows from a random row, about one screen. Each tree's
//! memory against its source is printed too (macOS only).
//!
//! Run with `cargo bench -p fe-syntax`; `cargo bench -p fe-syntax -- 10k`
//! runs only the small cases.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use fe_buffer::{Buffer, Origin};
use fe_syntax::{MAX_PARSE_BYTES, Syntax, language_by_name};
use rand::{Rng, SeedableRng, rngs::StdRng};

mod support;

const VISIBLE_ROWS: u32 = 60;

/// Whole units of `unit(n)` until the text has `lines` lines or `bytes` bytes.
fn repeat(lines: usize, bytes: usize, mut unit: impl FnMut(usize) -> String) -> String {
    let mut text = String::new();
    let mut count = 0;
    for n in 0.. {
        let next = unit(n);
        let next_lines = next.matches('\n').count();
        if count + next_lines > lines || text.len() + next.len() > bytes {
            break;
        }
        text.push_str(&next);
        count += next_lines;
    }
    text
}

fn rust(lines: usize, bytes: usize) -> String {
    repeat(lines, bytes, |n| {
        format!(
            r#"/// Documentation for record {n}, with `code` and a [link](https://example.com).
#[derive(Clone, Debug, PartialEq)]
pub struct Record{n} {{
    pub id: u64,
    name: String,
    values: Vec<f64>,
}}

impl Record{n} {{
    pub fn new(id: u64, name: &str) -> Self {{
        Self {{ id, name: name.to_owned(), values: Vec::with_capacity(16) }}
    }}

    /// Sums the values above `threshold`.
    pub fn total(&self, threshold: f64) -> f64 {{
        self.values.iter().copied().filter(|v| *v > threshold).sum()
    }}

    fn describe(&self) -> String {{
        match self.values.len() {{
            0 => format!("{{}} (empty)", self.name),
            1..=9 => format!("{{}}: {{}} values", self.name, self.values.len()),
            _ => {{
                let label = if self.id % 2 == 0 {{ "even" }} else {{ "odd" }};
                format!("{{}}: many values, {{label}} id {{}}", self.name, self.id)
            }}
        }}
    }}
}}

const LIMIT_{n}: usize = {n} * 2 + 'a' as usize; // a trailing comment

"#
        )
    })
}

fn json(lines: usize, bytes: usize) -> String {
    let mut text = String::from("[\n");
    text += &repeat(lines.saturating_sub(2), bytes.saturating_sub(4), |n| {
        format!(
            r#"  {{
    "id": {n},
    "name": "item {n}",
    "active": {},
    "score": {}.5,
    "tags": ["alpha", "beta", "gamma"],
    "owner": null,
    "nested": {{ "depth": 2, "path": "a/b/c" }}
  }},
"#,
            n % 2 == 0,
            n % 100,
        )
    });
    text.truncate(text.len() - 2);
    text + "\n]\n"
}

fn markdown(lines: usize, bytes: usize) -> String {
    repeat(lines, bytes, |n| {
        format!(
            r#"## Section {n}

Some *emphasis*, **strong** text, `inline code` and a [link](https://example.com/{n}).
A second line of the same paragraph, long enough to look like real prose.

- first item
- second item with `code`
  - nested item

> A quoted line.

```rust
fn example() {{}}
```

"#
        )
    })
}

/// A random offset between two letters, where typing extends a word.
fn inside_word(buffer: &Buffer, rng: &mut impl Rng) -> usize {
    let text = buffer.text();
    loop {
        let start = rng.random_range(0..text.len());
        let bytes = text.bytes_at(start);
        if let Some(ix) = bytes
            .windows(2)
            .position(|pair| pair.iter().all(u8::is_ascii_alphabetic))
        {
            return start + ix + 1;
        }
    }
}

fn benches(c: &mut Criterion) {
    type Corpus = fn(usize, usize) -> String;
    let languages: [(&str, Corpus); 3] = [("rust", rust), ("json", json), ("markdown", markdown)];
    let mut rng = StdRng::seed_from_u64(1);

    for (id, corpus) in languages {
        let language = language_by_name(id).expect("language is compiled in");
        let mut group = c.benchmark_group(id);
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(4));

        // Reparse and highlight where the editor highlights.
        let limit = MAX_PARSE_BYTES - 1024;
        let at_limit = format!("{}MB", MAX_PARSE_BYTES / (1024 * 1024));
        for (size, text) in [
            ("10k lines", corpus(10_000, usize::MAX)),
            (at_limit.as_str(), corpus(usize::MAX, limit)),
        ] {
            let mut buffer = Buffer::new(&text);
            let rows = buffer.text().row_count();
            eprintln!("{id} {size}: {} bytes, {rows} lines", text.len());

            let mut syntax = Syntax::new(language, &buffer.snapshot());
            group.bench_function(BenchmarkId::new("parse", size), |b| {
                b.iter(|| syntax.parse_job().run())
            });
            let before = support::allocated();
            syntax.finish(syntax.parse_job().run());
            let tree = support::allocated().saturating_sub(before);
            support::tree_memory(&format!("{id} {size}"), text.len(), tree);

            group.bench_function(BenchmarkId::new("highlight 60 rows", size), |b| {
                let text = buffer.text().clone();
                b.iter_batched(
                    || {
                        let first = rng.random_range(0..rows.saturating_sub(VISIBLE_ROWS));
                        text.row_start(first)..text.row_start(first + VISIBLE_ROWS)
                    },
                    |range| black_box(syntax.highlights(range)),
                    BatchSize::SmallInput,
                )
            });

            // Each keystroke is deleted again, untimed, so errors don't pile up.
            group.bench_function(BenchmarkId::new("reparse keystroke", size), |b| {
                let reparse = |buffer: &Buffer, syntax: &mut Syntax| {
                    syntax.edit(&buffer.snapshot());
                    let started = Instant::now();
                    assert!(syntax.parse_within(Duration::from_secs(60)));
                    started.elapsed()
                };
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let offset = inside_word(&buffer, &mut rng);
                        buffer.edit([(offset..offset, "x")], Origin::User);
                        total += reparse(&buffer, &mut syntax);
                        buffer.edit([(offset..offset + 1, "")], Origin::User);
                        reparse(&buffer, &mut syntax);
                    }
                    total
                })
            });
        }

        let text = corpus(1_000_000, usize::MAX);
        eprintln!("{id} 1M lines: {} bytes", text.len());
        let syntax = Syntax::new(language, &Buffer::new(&text).snapshot());
        group.bench_function(BenchmarkId::new("parse", "1M lines"), |b| {
            b.iter(|| syntax.parse_job().run())
        });
        group.finish();
    }
}

criterion_group!(syntax, benches);
criterion_main!(syntax);
