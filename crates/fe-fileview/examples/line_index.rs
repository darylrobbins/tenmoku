//! Times a full background line index of a 1 GiB file. Run in release:
//!
//! ```sh
//! cargo run --release -p fe-fileview --example line_index [-- PATH]
//! ```
//!
//! With no path it writes a 1 GiB file to `$TMPDIR` (the same 52-byte line as
//! the `first_screen` example, repeated) and deletes it afterwards, so the
//! page cache is warm. The file is created by `tempfile`, never at a guessable
//! path.
//!
//! RSS growth is the change in peak RSS from `getrusage` across the build. It
//! is Unix-only; elsewhere only the time is printed.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use fe_fileview::{FileView, IndexState, LineIndex};

const GIB: u64 = 1024 * 1024 * 1024;
const LINE: &str = "    let value = compute_something(…)?; // é 😀\n";

/// Writes `len` bytes of lines to a new temporary file, which is deleted when
/// it is dropped, however the example ends.
fn generate(len: u64) -> io::Result<tempfile::NamedTempFile> {
    let mut generated = tempfile::Builder::new()
        .prefix("fe-fileview-line-index-")
        .suffix(".txt")
        .tempfile()?;
    let mut block = LINE.repeat(1024 * 1024 / LINE.len()).into_bytes();
    let file = generated.as_file_mut();
    let mut written = 0;
    while written < len {
        block.truncate(block.len().min((len - written) as usize));
        file.write_all(&block)?;
        written += block.len() as u64;
    }
    file.sync_all()?;
    Ok(generated)
}

/// Peak resident set size in bytes.
#[cfg(unix)]
fn peak_rss() -> Option<u64> {
    // SAFETY: `getrusage` only writes the zeroed struct it is given.
    let usage = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    };
    // Bytes on macOS, KiB elsewhere.
    let scale = if cfg!(target_os = "macos") { 1 } else { 1024 };
    Some(usage.ru_maxrss as u64 * scale)
}

#[cfg(not(unix))]
fn peak_rss() -> Option<u64> {
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let generated;
    let path = match env::args_os().nth(1) {
        Some(path) => PathBuf::from(path),
        None => {
            println!("writing {} MiB to {}", GIB >> 20, env::temp_dir().display());
            generated = generate(GIB)?;
            generated.path().to_path_buf()
        }
    };

    let view = Arc::new(FileView::open(&path)?);
    let rss_before = peak_rss();
    let start = Instant::now();
    let index = LineIndex::build(Arc::clone(&view));
    let progress = loop {
        let progress = index.progress();
        if progress.state != IndexState::Building {
            break progress;
        }
        thread::sleep(Duration::from_micros(200));
    };
    let elapsed = start.elapsed();
    let rss_after = peak_rss();

    println!(
        "file: {} ({:.2} GiB)",
        path.display(),
        view.len() as f64 / GIB as f64
    );
    println!(
        "index: {:?} in {:.3} s, {} rows, {} checkpoints, {} KiB cached",
        progress.state,
        elapsed.as_secs_f64(),
        u64::from(progress.rows) + 1,
        index.checkpoints(),
        view.cached_bytes() >> 10,
    );
    if let (Some(before), Some(after)) = (rss_before, rss_after) {
        println!(
            "peak RSS {:.1} MB -> {:.1} MB (+{:.2} MB)",
            before as f64 / 1e6,
            after as f64 / 1e6,
            after.saturating_sub(before) as f64 / 1e6,
        );
    }
    Ok(())
}
