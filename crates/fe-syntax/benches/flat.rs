//! Highlighting cost on huge flat files, where one node has about a million
//! children: JSON arrays and a Markdown file of paragraphs, each filled to
//! just under its language's parse limit, the largest file the editor
//! highlights.
//!
//! A highlight is 60 rows, about one screen, at the start, middle and end of
//! the file. A cost that grows with position would show up there. An
//! unscrolled frame asks for the same rows again and is served from a cache.
//!
//! Also prints each tree's memory against its source (macOS only).
//!
//! Run with `cargo bench -p fe-syntax --bench flat`.

use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

mod support;

const VISIBLE_ROWS: u32 = 60;
/// More screens than [`Syntax`] caches highlights for.
const SCROLLED_ROWS: u32 = 32;

/// `header`, then lines from `line(n)` until the next would bring the text
/// within a kilobyte of `language`'s parse limit, then `footer`.
fn fill(language: &str, header: &str, footer: &str, line: impl Fn(usize) -> String) -> String {
    let limit = language_by_name(language)
        .expect("language is compiled in")
        .parse_limit();
    let mut text = String::from(header);
    for n in 0.. {
        let next = line(n);
        if text.len() + next.len() + footer.len() > limit - 1024 {
            break;
        }
        text += &next;
    }
    text + footer
}

fn benches(c: &mut Criterion) {
    let corpora = [
        (
            "json",
            "json numbers",
            fill("json", "[\n", "  0\n]\n", |n| format!("  {n},\n")),
        ),
        // The most elements that fit.
        (
            "json",
            "json zeros",
            fill("json", "[\n", "0\n]\n", |_| "0,\n".into()),
        ),
        (
            "markdown",
            "markdown paragraphs",
            fill("markdown", "", "", |n| format!("Paragraph {n}.\n\n")),
        ),
    ];

    let mut group = c.benchmark_group("flat");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(2));
    for (language, name, text) in corpora {
        let language = language_by_name(language).expect("language is compiled in");
        let buffer = Buffer::new(&text);
        let rows = buffer.text().row_count();
        let mut syntax = Syntax::new(language, &buffer.snapshot());
        let before = support::allocated();
        syntax.finish(syntax.parse_job().run());
        let tree = support::allocated().saturating_sub(before);
        support::tree_memory(&format!("{name}, {rows} lines"), text.len(), tree);

        // Each iteration scrolls a row, so it misses the highlight cache.
        let text = buffer.text();
        let screen = |first: u32| text.row_start(first)..text.row_start(first + VISIBLE_ROWS);
        let last = rows - VISIBLE_ROWS - SCROLLED_ROWS;
        for (at, first) in [("start", 0), ("middle", last / 2), ("end", last)] {
            let mut scrolled = 0;
            group.bench_function(BenchmarkId::new(name, at), |b| {
                b.iter(|| {
                    scrolled = (scrolled + 1) % SCROLLED_ROWS;
                    black_box(syntax.highlights(screen(first + scrolled)))
                })
            });
        }
        group.bench_function(BenchmarkId::new(name, "unscrolled"), |b| {
            b.iter(|| black_box(syntax.highlights(screen(0))))
        });
    }
    group.finish();
}

criterion_group!(flat, benches);
criterion_main!(flat);
