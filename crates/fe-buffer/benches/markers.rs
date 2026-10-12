//! Catching 100,000 anchored ranges up with a keystroke: `MarkerSet::sync`
//! shifts every marker after it with one gap change, where the anchors that
//! `DecorationSet` and `DiagnosticSet` keep are all remapped by
//! `BufferSnapshot::resolve_many`.
//!
//! Run with `cargo bench -p fe-buffer --bench markers`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fe_buffer::{Anchor, Bias, Buffer, MarkerSet, Origin};

const MARKERS: usize = 100_000;

/// 1 MiB of code-like text.
fn corpus() -> String {
    let line = "    let value = compute(argument, &mut state)?; // update\n";
    line.repeat((1 << 20) / line.len() + 1)[..1 << 20].to_string()
}

fn benches(c: &mut Criterion) {
    let mut buffer = Buffer::new(&corpus());
    let before = buffer.snapshot();
    // Spread evenly, none at offset 0, so the edit touches none of them.
    let stride = before.len() / MARKERS;
    let starts: Vec<usize> = (1..=MARKERS).map(|ix| ix * stride - stride / 2).collect();

    let mut markers = MarkerSet::new(&before);
    for (ix, &start) in starts.iter().enumerate() {
        markers.insert(start..start + 4, ix);
    }
    let anchors: Vec<Anchor> = (starts.iter())
        .map(|&start| before.anchor_at(start, Bias::Right))
        .collect();

    buffer.edit([(0..0, "x")], Origin::User);
    let after = buffer.snapshot();

    let mut group = c.benchmark_group("markers/100k after a keystroke at 0");
    group.bench_function("MarkerSet::sync", |b| {
        b.iter_batched(
            || markers.clone(),
            |mut markers| {
                markers.sync(black_box(&after)).unwrap();
                markers
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("resolve_many", |b| {
        b.iter(|| after.resolve_many(black_box(&anchors)))
    });
    group.finish();
}

criterion_group!(markers, benches);
criterion_main!(markers);
