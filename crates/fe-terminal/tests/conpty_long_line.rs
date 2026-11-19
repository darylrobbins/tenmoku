//! A session on Windows' ConPTY hands its output tap a line far wider than
//! the terminal whole, as the program wrote it (T1976): a read run's
//! cargo `compiler-artifact` line is read from the tap.
//!
//! The program is this test binary, run again with [`CHILD`] set, so the
//! line is written as cargo writes it: one console write from Rust's std.
#![cfg(windows)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fe_terminal::{GridSize, OutputTap, Terminal, TerminalEvent, TerminalOptions};

const DEADLINE: Duration = Duration::from_secs(60);

/// Set in the child's environment: what it prints.
const CHILD: &str = "FE_CONPTY_LONG_LINE_CHILD";

/// Lines printed before and after the long one: more than a screen, so the
/// line scrolls away as cargo's does under the lines that follow it.
const BEFORE: usize = 40;
const AFTER: usize = 400;

/// A 3,200-char line whose every 10 chars name their offset, so a cut
/// piece says where it came from.
fn long_line() -> String {
    let mut line = String::from("LONG:");
    for block in 0..320 {
        line.push_str(&format!("{:09}|", block * 10));
    }
    line
}

/// The child: prints its lines and leaves before libtest says more.
#[test]
fn child() {
    let Ok(mode) = std::env::var(CHILD) else {
        return;
    };
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    if mode == "flood" {
        for i in 0..BEFORE {
            writeln!(out, "before {i}").unwrap();
        }
    }
    writeln!(out, "{}", long_line()).unwrap();
    if mode == "flood" {
        for i in 0..AFTER {
            writeln!(out, "after {i}").unwrap();
        }
    }
    out.flush().unwrap();
    drop(out);
    std::process::exit(0);
}

/// What one run of the child in an 80-column session gives back.
struct Heard {
    /// All the tap heard.
    bytes: Vec<u8>,
    /// The grid's text, soft-wrapped rows joined.
    grid: String,
}

fn run(mode: &str) -> Heard {
    let exe = std::env::current_exe().unwrap();
    let options = TerminalOptions {
        cwd: std::env::temp_dir(),
        shell: Some((
            exe.to_string_lossy().into_owned(),
            ["child", "--exact", "--nocapture", "--test-threads=1"]
                .map(str::to_owned)
                .to_vec(),
        )),
        scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
        env: [(CHILD.to_owned(), mode.to_owned())].into(),
    };
    let size = GridSize {
        cols: 80,
        rows: 24,
        cell_width: 8,
        cell_height: 16,
    };
    let heard = Arc::new(Mutex::new(Vec::new()));
    let tap: OutputTap = Arc::new({
        let heard = heard.clone();
        move |bytes: &[u8]| heard.lock().unwrap().extend_from_slice(bytes)
    });
    let (terminal, events) = Terminal::spawn_tapped(options, size, Some(tap)).unwrap();
    let start = Instant::now();
    loop {
        match events.try_recv() {
            Ok(TerminalEvent::Exited(code)) => {
                assert_eq!(code, Some(0), "{mode}: the child failed");
                break;
            }
            Ok(_) => {}
            Err(_) => {
                assert!(start.elapsed() < DEADLINE, "{mode}: the child never exited");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    let bytes = heard.lock().unwrap().clone();
    Heard {
        bytes,
        grid: terminal.output_text(),
    }
}

/// The lines in `bytes`, as a read run's capture splits them: at `\n`, a
/// `\r` before it dropped.
fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

/// What the tap heard from the line's start on, escaped, for a failure.
fn excerpt(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let from = text.find("LONG:").map_or(0, |at| at.saturating_sub(200));
    let text: String = text[from..].chars().take(6000).collect();
    text.escape_debug().to_string()
}

#[test]
fn an_80_column_session_taps_a_3200_char_line_whole() {
    let line = long_line();
    let mut failures = Vec::new();
    for mode in ["quiet", "flood"] {
        let heard = run(mode);
        let lines = lines(&heard.bytes);
        if !lines.contains(&line) {
            let pieces: Vec<usize> = (lines.iter())
                .filter(|piece| piece.contains('|'))
                .map(String::len)
                .collect();
            failures.push(format!(
                "{mode}: the tap heard {} bytes, {} lines; lines with the long \
                 line's text are {pieces:?} chars long; the grid has it whole: {}\n\
                 heard from the line on: \"{}\"",
                heard.bytes.len(),
                lines.len(),
                heard.grid.contains(&line),
                excerpt(&heard.bytes),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
