//! M1 performance budgets (release, Apple Silicon):
//! - a 100 MB / 1M-line file loads well inside the 500 ms "interactive" budget;
//! - a keystroke edit in that file is far below a frame (8 ms p99 end to end);
//! - snapshots are O(1); anchor resolution scales with edits since the anchor
//!   (benched through 10, 100 and 1,000 edits; at most 12x per 10x edits).
//!
//! Run with `cargo bench -p fe-buffer`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fe_buffer::{Bias, Buffer, Origin, PointUtf16, Text};
use rand::{Rng, SeedableRng, rngs::StdRng};

/// About 100 MB of code-like text in 1M lines, with some non-ASCII.
fn corpus() -> String {
    let lines = [
        "    let value = compute_something(argument_one, argument_two, &mut state)?; // update the state",
        "    if value.is_empty() { return Err(Error::new(\"émpty value 😀 encountered while parsing\")); }",
        "fn handle_request(request: Request, context: &Context) -> Result<Response, HandlerError> {",
        "        .map(|item| item.name.to_uppercase()).filter(|name| !name.starts_with('_')).collect()",
        "}",
    ];
    let mut text = String::with_capacity(101 * 1024 * 1024);
    let mut ix = 0;
    while text.len() < 100 * 1024 * 1024 {
        text.push_str(lines[ix % lines.len()]);
        text.push('\n');
        ix += 1;
    }
    text
}

fn random_boundary(rng: &mut impl Rng, text: &Text) -> usize {
    text.clip_offset(rng.random_range(0..=text.len()), Bias::Left)
}

fn benches(c: &mut Criterion) {
    let source = corpus();
    let crlf = source.replace('\n', "\r\n");

    let mut load = c.benchmark_group("load");
    load.sample_size(10);
    load.bench_function("100MB", |b| b.iter(|| Buffer::new(black_box(&source))));
    load.bench_function("100MB crlf", |b| {
        b.iter(|| Buffer::from_file_contents(black_box(&crlf)))
    });
    load.finish();

    let mut rng = StdRng::seed_from_u64(1);
    let mut buffer = Buffer::new(&source);

    // `edit` clips offsets onto char boundaries, so raw random offsets are fine.
    let len = source.len();
    c.bench_function("edit/keystroke in 100MB", |b| {
        b.iter_batched(
            || rng.random_range(0..len),
            |offset| buffer.edit([(offset..offset, "x")], Origin::User),
            BatchSize::SmallInput,
        )
    });

    // Fill the default history so every edit past here trims one entry.
    let mut last = buffer.history_bytes(&Origin::User);
    loop {
        for _ in 0..1000 {
            let offset = rng.random_range(0..len);
            buffer.edit([(offset..offset, "x")], Origin::User);
        }
        let held = buffer.history_bytes(&Origin::User);
        if held <= last {
            break;
        }
        last = held;
    }
    c.bench_function("edit/keystroke with a full undo history in 100MB", |b| {
        b.iter_batched(
            || rng.random_range(0..len),
            |offset| buffer.edit([(offset..offset, "x")], Origin::User),
            BatchSize::SmallInput,
        )
    });

    c.bench_function("edit/100 cursors in 100MB", |b| {
        b.iter_batched(
            || {
                (0..100)
                    .map(|_| {
                        let offset = rng.random_range(0..len);
                        (offset..offset, "x")
                    })
                    .collect::<Vec<_>>()
            },
            |edits| buffer.edit(edits, Origin::User),
            BatchSize::SmallInput,
        )
    });

    c.bench_function("snapshot/clone", |b| {
        b.iter(|| black_box(buffer.snapshot()))
    });

    let text = buffer.text().clone();
    c.bench_function("convert/offset to LSP point in 100MB", |b| {
        b.iter_batched(
            || random_boundary(&mut rng, &text),
            |offset| text.offset_to_point_utf16(offset),
            BatchSize::SmallInput,
        )
    });
    c.bench_function("convert/LSP point to offset in 100MB", |b| {
        b.iter_batched(
            || {
                PointUtf16::new(
                    rng.random_range(0..text.row_count()),
                    rng.random_range(0..90),
                )
            },
            |point| text.point_utf16_to_offset(point),
            BatchSize::SmallInput,
        )
    });

    // 10k anchors (e.g. diagnostics) resolved after 10, 100 and 1,000 keystrokes.
    for edits in [10, 100, 1_000] {
        let mut buffer = Buffer::new(&source);
        let snapshot = buffer.snapshot();
        let anchors: Vec<_> = (0..10_000)
            .map(|_| snapshot.anchor_after(random_boundary(&mut rng, snapshot.text())))
            .collect();
        for _ in 0..edits {
            let offset = random_boundary(&mut rng, buffer.text());
            buffer.edit([(offset..offset, "x")], Origin::User);
        }
        let current = buffer.snapshot();
        c.bench_function(
            &format!("anchors/resolve 10k through {edits} edits, one by one"),
            |b| {
                b.iter(|| {
                    anchors
                        .iter()
                        .map(|anchor| current.resolve(anchor))
                        .sum::<usize>()
                })
            },
        );
        c.bench_function(
            &format!("anchors/resolve 10k through {edits} edits, batched"),
            |b| b.iter(|| current.resolve_many(&anchors)),
        );
    }

    // Undo of a keystroke deep in history, with later edits elsewhere.
    c.bench_function("undo/keystroke after 100 later edits", |b| {
        b.iter_batched(
            || {
                let mut buffer = Buffer::new("seed");
                buffer.edit(
                    [(0..0, "a")],
                    Origin::Ai {
                        agent: "a".into(),
                        session: "s".into(),
                    },
                );
                for ix in 0..100 {
                    let len = buffer.len();
                    buffer.edit(
                        [(len..len, if ix % 2 == 0 { "b" } else { "\n" })],
                        Origin::User,
                    );
                }
                buffer
            },
            |mut buffer| buffer.undo(|origin| origin.is_ai()).unwrap(),
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(m1, benches);
criterion_main!(m1);
