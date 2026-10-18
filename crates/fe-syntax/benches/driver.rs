//! What the UI thread spends in `SyntaxDriver` per frame while a background
//! parse really runs on another thread.
//!
//! The main thread plays the UI thread at 120 Hz. Each frame it types one
//! letter inside a random word, syncs the driver with the view's 1 ms budget,
//! finishes a parse that has landed, and syncs again, as the view does. The
//! three driver calls are timed together. `ParseJob`s run on `std::thread`s.
//!
//! Cases, 600 frames each:
//! - `rust/10k lines`: reparses fit the budget in the frame (the control);
//! - `rust/7 MiB`: just under Rust's parse limit, so every reparse goes to the
//!   background;
//! - `rust/20 MiB`: over the limit, so a window around the visible rows is
//!   parsed in the background; the viewport moves 60 rows every 10 frames.
//!
//! Budgets for the two big cases: p99 at most 2 ms, max at most 8 ms, with a
//! parse in flight at the start of at least half the frames.
//!
//! Run with `cargo bench -p fe-syntax --bench driver`.

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Origin};
use fe_syntax::{ParseJob, Parsed, Spawner, Syntax, SyntaxDriver, language_by_name};
use rand::{Rng, SeedableRng, rngs::StdRng};

const FRAMES: usize = 600;
const FRAME: Duration = Duration::from_nanos(8_333_333);
/// `SYNC_PARSE_BUDGET` in `crates/fe-gpui/src/view.rs`.
const SYNC_PARSE_BUDGET: Duration = Duration::from_millis(1);
const VISIBLE_ROWS: u32 = 60;
const MOVE_EVERY: usize = 10;

/// Runs each job on its own thread and sends the result back over a channel.
struct ThreadSpawner {
    results: Sender<Parsed>,
    spawned: usize,
    /// Jobs a worker thread is parsing right now: not yet done, whether or not
    /// the UI thread has picked up the result.
    running: Arc<AtomicUsize>,
}

impl Spawner for ThreadSpawner {
    // `ParseJob` has no cancel flag, so a dropped handle just detaches.
    type Task = JoinHandle<()>;

    fn spawn(&mut self, job: ParseJob) -> Self::Task {
        self.spawned += 1;
        let results = self.results.clone();
        let running = Arc::clone(&self.running);
        thread::spawn(move || {
            running.fetch_add(1, Ordering::SeqCst);
            let parsed = job.run();
            running.fetch_sub(1, Ordering::SeqCst);
            let _ = results.send(parsed);
        })
    }
}

/// Whole Rust items until the text has `bytes` bytes at most.
fn rust(bytes: usize, lines: usize) -> String {
    let mut text = String::new();
    let mut count = 0;
    for n in 0.. {
        let next = format!(
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
        );
        let next_lines = next.matches('\n').count();
        if count + next_lines > lines || text.len() + next.len() > bytes {
            break;
        }
        text.push_str(&next);
        count += next_lines;
    }
    text
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

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn run_case(name: &str, text: &str) {
    let language = language_by_name("rust").expect("language is compiled in");
    let mut buffer = Buffer::new(text);
    let rows = buffer.text().row_count();
    let mut rng = StdRng::seed_from_u64(1);

    let (results, landed) = mpsc::channel();
    let mut spawner = ThreadSpawner {
        results,
        spawned: 0,
        running: Arc::new(AtomicUsize::new(0)),
    };
    let mut driver =
        SyntaxDriver::new(Syntax::new(language, &buffer.snapshot()), SYNC_PARSE_BUDGET);

    let visible_at = |buffer: &Buffer, first: u32| -> Range<usize> {
        let text = buffer.text();
        text.row_start(first)..text.row_start((first + VISIBLE_ROWS).min(rows - 1))
    };
    let mut first_row = 0;
    let mut visible = visible_at(&buffer, first_row);

    // The first parse of the file isn't a typing frame: let it land.
    let snapshot = buffer.snapshot();
    driver.sync(&snapshot, visible.clone(), &mut spawner);
    let mut received = 0;
    while driver.is_parsing() {
        let parsed = landed.recv().expect("parse thread sends");
        received += 1;
        driver.finish(parsed, &mut spawner);
    }
    while let Ok(parsed) = landed.try_recv() {
        received += 1;
        driver.finish(parsed, &mut spawner);
    }

    let mut times = Vec::with_capacity(FRAMES);
    let mut in_flight = 0;
    let mut parses_landed = 0;
    let mut skipped = 0;
    let mut deadline = Instant::now();
    for frame in 0..FRAMES {
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        // A frame that overran skips the ticks it missed rather than catching
        // up with back-to-back frames, as a display would.
        deadline += FRAME;
        while deadline <= Instant::now() {
            deadline += FRAME;
            skipped += 1;
        }
        // Whether a worker is parsing as the frame starts, before it types.
        if spawner.running.load(Ordering::SeqCst) > 0 {
            in_flight += 1;
        }
        if frame > 0 && frame % MOVE_EVERY == 0 {
            first_row = (first_row + VISIBLE_ROWS) % rows.saturating_sub(VISIBLE_ROWS).max(1);
            visible = visible_at(&buffer, first_row);
        }
        let offset = inside_word(&buffer, &mut rng);
        buffer.edit([(offset..offset, "x")], Origin::User);
        let snapshot = buffer.snapshot();

        let started = Instant::now();
        driver.sync(&snapshot, visible.clone(), &mut spawner);
        let synced = started.elapsed();
        let mut landed_now = false;
        while let Ok(parsed) = landed.try_recv() {
            received += 1;
            parses_landed += 1;
            landed_now = true;
            driver.finish(parsed, &mut spawner);
        }
        let finished = started.elapsed();
        if landed_now {
            driver.sync(&snapshot, visible.clone(), &mut spawner);
        }
        let total = started.elapsed();
        if std::env::var_os("DRIVER_TRACE").is_some() && total > Duration::from_millis(2) {
            eprintln!(
                "frame {frame}: sync {synced:?}, finish {:?}, catch-up sync {:?}",
                finished - synced,
                total - finished
            );
        }
        times.push(total.as_secs_f64() * 1e3);
    }

    // Let the parses still running finish before the next case starts.
    drop(driver);
    while received < spawner.spawned {
        landed.recv().expect("parse thread sends");
        received += 1;
    }

    times.sort_by(f64::total_cmp);
    println!(
        "{name}: {FRAMES} frames, {in_flight} with a parse running at their start, {parses_landed} parses landed, \
         {skipped} ticks skipped, \
         p50 {:.3} ms, p99 {:.3} ms, max {:.3} ms",
        percentile(&times, 0.5),
        percentile(&times, 0.99),
        times[times.len() - 1],
    );
}

fn main() {
    let language = language_by_name("rust").expect("language is compiled in");
    let limit = language.parse_limit();
    for (name, text) in [
        ("rust/10k lines", rust(usize::MAX, 10_000)),
        ("rust/7 MiB", rust(limit - 1024, usize::MAX)),
        ("rust/20 MiB", rust(20 << 20, usize::MAX)),
    ] {
        eprintln!("{name}: {} bytes", text.len());
        run_case(name, &text);
    }
}
