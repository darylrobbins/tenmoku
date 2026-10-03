//! Measures how much memory the edit log and undo history hold on to.
//! Run: `cargo run --release -p fe-buffer --example history_memory`
//!
//! Release-run numbers (Apple Silicon, RSS via `ps`, sampled with inputs alive):
//!
//! - Long session (1 MB file, 200k single-character user transactions, 10%
//!   deletions): +98.8 MB, 494 B per transaction. After undoing all 200k,
//!   memory grew a further 8.7 MB (538 B per transaction in total): nothing is
//!   reclaimed, because the log keeps the undo batches too.
//! - Replace-all (33 MB file, 507,692 occurrences per pass, 6 -> 8 bytes, one
//!   batch per pass): pass 1 +138.8 MB (45.6 B per replaced byte); passes 2-5
//!   +65.1-65.2 MB each (21.4 B per replaced byte). Total +399.5 MB, 12x the
//!   file, for five passes. Pass 1 is higher partly because it also pays for
//!   first-touch allocator and tree growth.
//! - After undoing the five passes: +16.3 MB more, again nothing reclaimed.
//!
//! Memory grows linearly with edits and is never released. Per-edit cost is
//! roughly 21 B per replaced byte (about 128 B per edit) in a batch, and
//! about 500 B per single-character transaction.

use fe_buffer::{Buffer, Origin};

fn resident_bytes() -> usize {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0)
        * 1024
}

fn undo_all(buffer: &mut Buffer) -> usize {
    let mut count = 0;
    while buffer.undo(|_| true).expect("undo").is_some() {
        count += 1;
    }
    count
}

fn mb(bytes: isize) -> f64 {
    bytes as f64 / 1e6
}

fn growth(after: usize, before: usize) -> isize {
    after as isize - before as isize
}

/// 1 MB file, then 200k single-character user transactions at moving
/// positions, every tenth one a deletion.
fn long_session() {
    const TRANSACTIONS: usize = 200_000;
    let line = "    let value = compute_something(argument_one, argument_two)?;\n";
    let source = line.repeat(1_000_000 / line.len());
    let mut buffer = Buffer::new(&source);

    let start = resident_bytes();
    let mut position = 0usize;
    for i in 0..TRANSACTIONS {
        // Stride by a prime so edits land all over the file.
        position = (position + 7919) % buffer.len();
        if i % 10 == 9 {
            buffer.edit([(position..position + 1, "")], Origin::User);
        } else {
            buffer.edit([(position..position, "x")], Origin::User);
        }
    }
    let typed = resident_bytes();
    let undone = undo_all(&mut buffer);
    let after_undo = resident_bytes();
    println!(
        "long session: {TRANSACTIONS} transactions, +{:.1} MB ({:.0} B/transaction); \
         after undoing {undone}: {:+.1} MB vs. typed ({:.0} B/transaction left)",
        mb(growth(typed, start)),
        growth(typed, start) as f64 / TRANSACTIONS as f64,
        mb(growth(after_undo, typed)),
        growth(after_undo, start) as f64 / TRANSACTIONS as f64,
    );
    std::hint::black_box((&source, &buffer));
}

/// 33 MB file, five passes each replacing every 6-byte word with an 8-byte
/// one in a single batch.
fn replace_all() {
    const PASSES: usize = 5;
    let line = "    let value = foobar(argument_one, argument_two, &mut state)?;\n";
    let word_at = line.find("foobar").unwrap();
    let occurrences = 33_000_000 / line.len();
    let source = line.repeat(occurrences);
    let mut buffer = Buffer::new(&source);
    println!(
        "replace-all: file {:.1} MB, {occurrences} occurrences per pass",
        source.len() as f64 / 1e6
    );

    let start = resident_bytes();
    let mut previous = start;
    for pass in 0..PASSES {
        // Each pass grows every line by 2 bytes, so offsets are computed
        // rather than found, which keeps a second copy of the text out of
        // the measurement.
        let line_len = line.len() + 2 * pass;
        let edits: Vec<_> = (0..occurrences)
            .map(|n| {
                let at = n * line_len + word_at;
                (at..at + 6, "foobar12")
            })
            .collect();
        buffer.edit(edits, Origin::User);
        let now = resident_bytes();
        println!(
            "  pass {}: +{:.1} MB ({:.2} B per replaced byte)",
            pass + 1,
            mb(growth(now, previous)),
            growth(now, previous) as f64 / (occurrences * 6) as f64,
        );
        previous = now;
    }
    let done = resident_bytes();
    let undone = undo_all(&mut buffer);
    let after_undo = resident_bytes();
    println!(
        "  total +{:.1} MB; after undoing {undone}: {:+.1} MB vs. before undo",
        mb(growth(done, start)),
        mb(growth(after_undo, done)),
    );
    std::hint::black_box((&source, &buffer));
}

fn main() {
    long_session();
    replace_all();
}
