//! Display map budgets on a 1M-line file (release, Apple Silicon): a
//! keystroke re-wraps only its row, far inside the 8 ms p99 keystroke-to-paint
//! budget; wrapping the whole file (turning soft wrap on, resizing) is
//! measured to see how far it is from a frame.
//!
//! Run with `cargo bench -p fe-display`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fe_buffer::{Buffer, Origin};
use fe_display::{DisplayMap, Monospace};
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

fn benches(c: &mut Criterion) {
    let mut buffer = Buffer::new(&corpus());
    let mut map = DisplayMap::new(buffer.current(), 4);

    let mut wrap = c.benchmark_group("wrap");
    wrap.sample_size(10);
    wrap.bench_function("1M lines at 80 columns", |b| {
        b.iter(|| {
            map.set_wrap_width(None, &mut Monospace);
            map.set_wrap_width(Some(black_box(80.)), &mut Monospace);
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
}

criterion_group!(display, benches);
criterion_main!(display);
