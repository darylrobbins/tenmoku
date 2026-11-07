//
// Unix only: the emulator is fed `/bin/sh` scripts. It is the same code on
// Windows, where ConPTY re-renders the stream (see fe-terminal's conpty.rs).
#![cfg(unix)]

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fe_terminal::{
    GridSize, Link, LinkTarget, Snapshot, Terminal, TerminalEvent, TerminalOptions, link_at,
};

const DEADLINE: Duration = Duration::from_secs(5);

struct Session {
    terminal: Terminal,
    _events: async_channel::Receiver<TerminalEvent>,
}

impl Session {
    /// Runs `/bin/sh -c script` in `cwd` on a 120×12 grid.
    fn sh(script: &str, cwd: &Path) -> Session {
        let options = TerminalOptions {
            cwd: cwd.to_owned(),
            shell: Some((
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            )),
            scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
            env: Default::default(),
        };
        let size = GridSize {
            cols: 120,
            rows: 12,
            cell_width: 8,
            cell_height: 16,
        };
        let (terminal, events) = Terminal::spawn(options, size).expect("spawn /bin/sh");
        Session {
            terminal,
            _events: events,
        }
    }

    /// Waits up to 5 s for the screen to show `text`, and returns it.
    fn wait_for_text(&self, text: &str) -> Snapshot {
        let start = Instant::now();
        loop {
            let snapshot = self.terminal.snapshot();
            if snapshot.text().contains(text) {
                return snapshot;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {text}; screen:\n{}",
                snapshot.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// A script printing each of `lines`, then waiting on stdin.
fn print_lines(lines: &[&str]) -> String {
    let mut script = "printf '%s\\n'".to_owned();
    for line in lines {
        script.push_str(&format!(" '{line}'"));
    }
    script.push_str("; read -r x");
    script
}

/// The row reading `text`.
fn row_of(snapshot: &Snapshot, text: &str) -> usize {
    (0..snapshot.rows.len())
        .find(|&row| snapshot.row_text(row) == text)
        .unwrap_or_else(|| panic!("no row {text:?} in:\n{}", snapshot.text()))
}

/// The columns of `part` in row `row` (ASCII text).
fn cols_of(snapshot: &Snapshot, row: usize, part: &str) -> Range<usize> {
    let start = snapshot.row_text(row).find(part).expect("part in row");
    start..start + part.len()
}

/// Asserts every cell of `part` in the row reading `text` links to `target`,
/// covering exactly `part`, and the cells either side don't.
fn assert_link(snapshot: &Snapshot, cwd: &Path, text: &str, part: &str, target: &LinkTarget) {
    let row = row_of(snapshot, text);
    let cols = cols_of(snapshot, row, part);
    for col in cols.clone() {
        assert_eq!(
            link_at(snapshot, row, col, cwd),
            Some(Link {
                row,
                cols: cols.clone(),
                target: target.clone(),
            }),
            "col {col} of {text:?}"
        );
    }
    for col in [cols.start.wrapping_sub(1), cols.end] {
        if col < snapshot.cols {
            assert_eq!(
                link_at(snapshot, row, col, cwd),
                None,
                "col {col} of {text:?}"
            );
        }
    }
}

fn file(path: PathBuf, line: Option<u32>, column: Option<u32>) -> LinkTarget {
    LinkTarget::File { path, line, column }
}

#[test]
fn quoted_paths_take_a_line_and_column_in_words() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cwd = dir.path();
    std::fs::create_dir(cwd.join("src")).expect("src dir");
    std::fs::write(cwd.join("src/tool.py"), "").expect("file");
    let abs = cwd.join("src/tool.py");
    let abs_line = format!("  File \"{}\", line 3, column 9", abs.display());
    let lines = [
        "  File \"src/tool.py\", line 12, in <module>",
        abs_line.as_str(),
        "\"src/tool.py\", line 4, col 2",
        "  File \"src/tool.py\", in <module>",
    ];
    let session = Session::sh(&print_lines(&lines), cwd);
    let snapshot = session.wait_for_text("in <module>\n");
    let target = cwd.join("src/tool.py");
    assert_link(
        &snapshot,
        cwd,
        lines[0],
        "src/tool.py\", line 12",
        &file(target.clone(), Some(12), None),
    );
    assert_link(
        &snapshot,
        cwd,
        lines[1],
        &format!("{}\", line 3, column 9", abs.display()),
        &file(abs.clone(), Some(3), Some(9)),
    );
    assert_link(
        &snapshot,
        cwd,
        lines[2],
        "src/tool.py\", line 4, col 2",
        &file(target.clone(), Some(4), Some(2)),
    );
    assert_link(
        &snapshot,
        cwd,
        lines[3],
        "src/tool.py",
        &file(target, None, None),
    );
}
