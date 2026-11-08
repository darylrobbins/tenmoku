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

fn url(url: &str) -> LinkTarget {
    LinkTarget::Url(url.to_owned())
}

fn file(path: PathBuf, line: Option<u32>, column: Option<u32>) -> LinkTarget {
    LinkTarget::File { path, line, column }
}

#[test]
fn urls() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cwd = dir.path();
    let lines = [
        "see https://example.com/a?b=1).",
        "(https://x.test/z)",
        "https://en.wikipedia.org/wiki/Rust_(language)",
        "mail mailto:me@x.test, or \"http://q.test/p\" and file:///tmp/a%20b",
    ];
    let session = Session::sh(&print_lines(&lines), cwd);
    let snapshot = session.wait_for_text("file:///tmp");

    let part = "https://example.com/a?b=1";
    assert_link(&snapshot, cwd, lines[0], part, &url(part));
    let row = row_of(&snapshot, lines[0]);
    for col in 0..4 {
        assert_eq!(link_at(&snapshot, row, col, cwd), None, "col {col}");
    }

    let part = "https://x.test/z";
    assert_link(&snapshot, cwd, lines[1], part, &url(part));

    assert_link(&snapshot, cwd, lines[2], lines[2], &url(lines[2]));

    let part = "mailto:me@x.test";
    assert_link(&snapshot, cwd, lines[3], part, &url(part));
    let part = "http://q.test/p";
    assert_link(&snapshot, cwd, lines[3], part, &url(part));
    let target = file(PathBuf::from("/tmp/a b"), None, None);
    assert_link(&snapshot, cwd, lines[3], "file:///tmp/a%20b", &target);

    // Blank cells past the text, and rows off the grid.
    assert_eq!(link_at(&snapshot, row, 100, cwd), None);
    assert_eq!(link_at(&snapshot, 99, 0, cwd), None);
}

#[test]
fn paths() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cwd = dir.path();
    std::fs::create_dir(cwd.join("src")).expect("mkdir src");
    std::fs::write(cwd.join("src/main.rs"), "fn main() {}\n").expect("write main.rs");
    let main = cwd.join("src/main.rs");
    let absolute = main.to_str().expect("utf-8 temp path").to_owned();
    let lines = [
        "src/main.rs:12:5: error".to_owned(),
        "  --> src/main.rs(3,7)".to_owned(),
        "./src/main.rs".to_owned(),
        format!("in {absolute}."),
        "see src/main.rs:9".to_owned(),
        "dir src/ too".to_owned(),
        "missing.rs:1 nope".to_owned(),
    ];
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let session = Session::sh(&print_lines(&line_refs), cwd);
    let snapshot = session.wait_for_text("nope");

    let target = file(main.clone(), Some(12), Some(5));
    assert_link(&snapshot, cwd, &lines[0], "src/main.rs:12:5", &target);
    let row = row_of(&snapshot, &lines[0]);
    let error = cols_of(&snapshot, row, "error");
    assert_eq!(link_at(&snapshot, row, error.start, cwd), None);

    let target = file(main.clone(), Some(3), Some(7));
    assert_link(&snapshot, cwd, &lines[1], "src/main.rs(3,7)", &target);

    let target = file(main.clone(), None, None);
    assert_link(&snapshot, cwd, &lines[2], "./src/main.rs", &target);
    // The sentence's full stop isn't part of the path.
    assert_link(&snapshot, cwd, &lines[3], &absolute, &target);

    let target = file(main.clone(), Some(9), None);
    assert_link(&snapshot, cwd, &lines[4], "src/main.rs:9", &target);
    let target = file(cwd.join("src"), None, None);
    assert_link(&snapshot, cwd, &lines[5], "src/", &target);

    let row = row_of(&snapshot, &lines[6]);
    for col in 0..snapshot.cols {
        assert_eq!(link_at(&snapshot, row, col, cwd), None, "col {col}");
    }
}

#[test]
fn osc8() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cwd = dir.path();
    let script = concat!(
        r"printf 'see \033]8;;https://a.test/x\033\\label\033]8;;\033\\ now\n'; ",
        r"printf 'open \033]8;id=1;file:///tmp/notes.txt\033\\the notes\033]8;;\033\\\n'; ",
        "read -r x",
    );
    let session = Session::sh(script, cwd);
    let snapshot = session.wait_for_text("the notes");

    let row = row_of(&snapshot, "see label now");
    let label = cols_of(&snapshot, row, "label");
    let link = snapshot.rows[row][label.start].link;
    assert!(link.is_some(), "label has no link");
    for col in label.clone() {
        assert_eq!(snapshot.rows[row][col].link, link, "col {col}");
    }
    assert_eq!(snapshot.rows[row][label.start - 1].link, None);
    assert_eq!(snapshot.rows[row][label.end].link, None);
    let index = link.unwrap() as usize;
    assert_eq!(snapshot.links[index], "https://a.test/x");
    assert!(snapshot.links.contains(&"file:///tmp/notes.txt".to_owned()));

    // The hyperlink wins over the text, which isn't a URL at all.
    assert_link(
        &snapshot,
        cwd,
        "see label now",
        "label",
        &url("https://a.test/x"),
    );
    let target = file(PathBuf::from("/tmp/notes.txt"), None, None);
    assert_link(&snapshot, cwd, "open the notes", "the notes", &target);
}
