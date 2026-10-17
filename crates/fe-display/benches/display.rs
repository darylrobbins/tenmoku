//! Display map budgets on a 1M-line file (release, Apple Silicon): a
//! keystroke re-wraps only its row, far inside the 8 ms p99 keystroke-to-paint
//! budget; a new wrap width (turning soft wrap on, resizing) marks every row
//! pending and wraps one slice of [`WRAP_SLICE_BYTES`] around the viewport
//! per frame, which has to stay well inside a frame.
//!
//! A keystroke also brings every fold's hidden rows up to date:
//! `sync/keystroke, 1M lines wrapped, 10k folds` stays under 0.5 ms, and
//! `sync/keystroke, 1M lines wrapped, 200k folds` (Fold All on such a file)
//! and `fold/one more fold among 200k, wrapped` are what a fold-heavy file
//! costs per keystroke and per fold.
//!
//! Run with `cargo bench -p fe-display`.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{BatchSize, Bencher, Criterion, criterion_group, criterion_main};
use fe_buffer::{Buffer, Origin};
use fe_display::{DisplayMap, Monospace, WRAP_SLICE_BYTES};
use rand::{Rng, SeedableRng, rngs::StdRng};

/// 1M lines of code-like text, about 75 MB, most wider than 80 columns.
fn corpus() -> String {
    let lines = [
        "    let value = compute_something(argument_one, argument_two, &mut state)?; // update the state",
        "    if value.is_empty() { return Err(Error::new(\"émpty value 😀 encountered while parsing\")); }",
        "fn handle_request(request: Request, context: &Context) -> Result<Response, HandlerError> {",
        "        .map(|item| item.name.to_uppercase()).filter(|name| !name.starts_with('_')).collect()",
        "}",
    ];
    let mut text = String::with_capacity(80 * 1024 * 1024);
    for ix in 0..1_000_000 {
        text.push_str(lines[ix % lines.len()]);
        text.push('\n');
    }
    text
}

/// The viewport's first buffer row.
const AROUND: u32 = 500_000;

fn benches(c: &mut Criterion) {
    let mut buffer = Buffer::new(&corpus());
    // The keystrokes would fill the undo history, and trimming a full one
    // costs more per edit than the display work measured here.
    buffer.set_history_limit(usize::MAX);
    let mut map = DisplayMap::new(buffer.current(), 4);

    let mut wrap = c.benchmark_group("wrap");
    wrap.bench_function("set width + one slice, 1M lines", |b| {
        b.iter(|| {
            map.set_wrap_width(None, &mut Monospace);
            map.set_wrap_width(Some(black_box(80.)), &mut Monospace);
            map.wrap_pending(AROUND, WRAP_SLICE_BYTES, &mut Monospace)
        })
    });
    wrap.bench_function("one slice, 1M lines", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                map.set_wrap_width(None, &mut Monospace);
                map.set_wrap_width(Some(80.), &mut Monospace);
                let start = Instant::now();
                black_box(map.wrap_pending(AROUND, WRAP_SLICE_BYTES, &mut Monospace));
                total += start.elapsed();
            }
            total
        })
    });
    wrap.sample_size(10);
    wrap.bench_function("all pending rows in slices, 1M lines at 80 columns", |b| {
        b.iter(|| {
            map.set_wrap_width(None, &mut Monospace);
            map.set_wrap_width(Some(black_box(80.)), &mut Monospace);
            while map.wrap_pending(AROUND, WRAP_SLICE_BYTES, &mut Monospace) {}
        })
    });
    wrap.finish();

    // Some folds to keep up to date as well.
    for row in (0..1_000_000).step_by(50_000) {
        map.fold(row..=row + 20);
    }
    let mut rng = StdRng::seed_from_u64(1);
    let len = buffer.len();
    for (name, width) in [("unwrapped", None), ("wrapped", Some(80.))] {
        map.set_wrap_width(width, &mut Monospace);
        map.wrap_pending(0, usize::MAX, &mut Monospace);
        c.bench_function(&format!("sync/keystroke, 1M lines {name}"), |b| {
            b.iter_batched(
                || rng.random_range(0..len),
                |offset| {
                    buffer.edit([(offset..offset, "x")], Origin::User);
                    map.sync(buffer.current(), &mut Monospace);
                },
                BatchSize::SmallInput,
            )
        });
    }

    // Many folds: a 4-row fold every `step` rows, typed on outside them so
    // keystrokes keep them.
    let mut keystroke = |b: &mut Bencher, map: &mut DisplayMap, step: u32| {
        b.iter_batched(
            || {
                let row = rng.random_range(0..1_000_000);
                if (1..4).contains(&(row % step)) {
                    row - row % step
                } else {
                    row
                }
            },
            |row| {
                let offset = buffer.text().row_start(row);
                buffer.edit([(offset..offset, "x")], Origin::User);
                map.sync(buffer.current(), &mut Monospace);
            },
            BatchSize::SmallInput,
        )
    };
    let fold_every = |map: &mut DisplayMap, step: u32| {
        map.unfold_all();
        map.fold_many(
            (0..1_000_000)
                .step_by(step as usize)
                .map(|row| row..=row + 3),
        );
    };
    fold_every(&mut map, 100);
    assert_eq!(map.folds().len(), 10_000);
    for (name, width) in [("unwrapped", None), ("wrapped", Some(80.))] {
        map.set_wrap_width(width, &mut Monospace);
        map.wrap_pending(0, usize::MAX, &mut Monospace);
        c.bench_function(
            &format!("sync/keystroke, 1M lines {name}, 10k folds"),
            |b| keystroke(b, &mut map, 100),
        );
    }

    fold_every(&mut map, 5);
    assert_eq!(map.folds().len(), 200_000);
    let mut sync = c.benchmark_group("sync");
    sync.sample_size(10);
    sync.bench_function("keystroke, 1M lines wrapped, 200k folds", |b| {
        keystroke(b, &mut map, 5)
    });
    sync.finish();
    let mut fold = c.benchmark_group("fold");
    fold.sample_size(10);
    fold.bench_function("one more fold among 200k, wrapped", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for ix in 0..iters {
                // A fold of hidden rows, nested in their header's fold; then
                // back to the header's fold alone, untimed.
                let header = (ix * 5_003 % 200_000) as u32 * 5;
                let start = Instant::now();
                black_box(map.fold(header + 1..=header + 2));
                total += start.elapsed();
                map.unfold(header + 1..=header + 2);
                map.fold(header..=header + 3);
            }
            total
        })
    });
    fold.finish();
}

criterion_group!(display, benches);
criterion_main!(display);
