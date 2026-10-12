//! Keystroke-to-frame latency, by stage: a synthesized keystroke goes through
//! the real keymap and input handling into a focused `EditorView`, and is
//! timed until the frame showing its effect has been drawn and presented.
//! The cases and stages are in `harness.rs`.
//!
//! Everything runs headless: GPUI's test window with the real macOS text
//! system and its headless Metal renderer. No window is opened on screen, so
//! a locked screen doesn't matter.
//!
//! Read the numbers against the plan's typing target: keystroke to paint
//! under 4 ms at p50 and 8 ms at p99. Nothing asserts it. Criterion's
//! statistics cover the total; after each case, p50 and p99 of each stage
//! and of the total over its measured samples are printed too.
//!
//! Not covered: delivering the OS event (`NSEvent` to GPUI), waiting for
//! vsync, the compositor, scan-out and the GPU's own time, which need a
//! real screen.
//!
//! Run with `cargo bench -p fe-gpui --bench keystroke` (about 20 seconds after
//! the build); `-- --test` runs each case once, and `-- type/small` just one.

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use harness::{Harness, STAGES, Sample};

// Shared with the `keystroke_latency` test, which uses the rest.
#[allow(dead_code)]
mod harness;

const SAMPLE_SIZE: usize = 50;

fn benches(c: &mut Criterion) {
    // Text shaping needs the main thread, which a `harness = false` bench has.
    let text_system = gpui_platform::current_platform(true).text_system();
    let mut group = c.benchmark_group("keystroke");
    group.sample_size(SAMPLE_SIZE);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for case in harness::cases() {
        // Built on first use, so a filtered-out case costs nothing.
        let mut harness = None;
        // One batch per call: criterion's warm-up calls come first, then one
        // call per measured sample.
        let mut batches: Vec<Vec<Sample>> = Vec::new();
        group.bench_function(case.name, |b| {
            let harness = harness.get_or_insert_with(|| {
                Harness::new(
                    text_system.clone(),
                    gpui_platform::current_headless_renderer,
                    &case,
                )
            });
            b.iter_custom(|iters| {
                let mut batch = Vec::new();
                for _ in 0..iters {
                    let sample = harness
                        .keystroke(case.keys)
                        .unwrap_or_else(|err| panic!("{}: {err:?}", case.name));
                    harness.reset();
                    batch.push(sample);
                }
                let total = batch.iter().map(Sample::total).sum();
                batches.push(batch);
                total
            })
        });
        let measured = batches[batches.len().saturating_sub(SAMPLE_SIZE)..].concat();
        if !measured.is_empty() {
            print_percentiles(case.name, &measured);
        }
    }
    group.finish();
}

/// p50 and p99 of each stage and of the total, in milliseconds.
fn print_percentiles(name: &str, samples: &[Sample]) {
    let percentiles = |mut values: Vec<Duration>| {
        values.sort();
        let at = |p: usize| values[(values.len() - 1) * p / 100].as_secs_f64() * 1e3;
        format!("{:>8.3} {:>8.3}", at(50), at(99))
    };
    eprintln!("{name}: {} samples, ms        p50      p99", samples.len());
    for (stage, label) in STAGES.iter().enumerate() {
        let values = samples
            .iter()
            .map(|sample| sample.stages()[stage])
            .collect();
        eprintln!("  {label:<26}{}", percentiles(values));
    }
    let totals = samples.iter().map(Sample::total).collect();
    eprintln!("  {:<26}{}", "total", percentiles(totals));
}

criterion_group!(keystroke, benches);
criterion_main!(keystroke);
