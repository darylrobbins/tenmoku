//
// Unix only: the emulator is fed `/bin/sh` scripts. It is the same code on
// Windows, where ConPTY re-renders the stream (see fe-terminal's conpty.rs).
#![cfg(unix)]

use std::time::{Duration, Instant};

use fe_terminal::{
    GridPoint, GridSize, SelectionSpan, Snapshot, Terminal, TerminalEvent, TerminalMatch,
    TerminalOptions, TerminalQuery,
};

const DEADLINE: Duration = Duration::from_secs(5);

/// Prints `line 01` to `line 40`, then waits on stdin.
const FORTY_LINES: &str =
    "i=1; while [ $i -le 40 ]; do printf 'line %02d\\n' $i; i=$((i+1)); done; read -r x";

struct Session {
    terminal: Terminal,
    _events: async_channel::Receiver<TerminalEvent>,
    _dir: tempfile::TempDir,
}

impl Session {
    /// Runs `/bin/sh -c script` on a `cols`×`rows` grid.
    fn sh(script: &str, cols: u16, rows: u16) -> Session {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = TerminalOptions {
            cwd: dir.path().to_owned(),
            shell: Some((
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            )),
            scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
            env: Default::default(),
        };
        let size = GridSize {
            cols,
            rows,
            cell_width: 8,
            cell_height: 16,
        };
        let (terminal, events) = Terminal::spawn(options, size).expect("spawn /bin/sh");
        Session {
            terminal,
            _events: events,
            _dir: dir,
        }
    }

    /// Waits up to 5 s for `done` to hold on the bottom of the grid.
    fn wait_for(&self, what: &str, done: impl Fn(&Snapshot) -> bool) {
        let start = Instant::now();
        loop {
            let snapshot = self.terminal.snapshot();
            if done(&snapshot) {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {what}; screen:\n{}",
                snapshot.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_text(&self, text: &str) {
        self.wait_for(text, |snapshot| snapshot.text().contains(text));
    }

    fn find(&self, query: &TerminalQuery, forward: bool) -> TerminalMatch {
        self.terminal
            .find(query, forward)
            .expect("query compiles")
            .unwrap_or_else(|| panic!("no match for {query:?}"))
    }

    fn count(&self, query: &TerminalQuery) -> usize {
        self.terminal
            .find_all_visible(query)
            .expect("query compiles")
            .len()
    }
}

fn point(line: i32, col: usize) -> GridPoint {
    GridPoint { line, col }
}

/// Asserts the snapshot shows `found` selected, on one row reading `text`.
fn assert_selected(snapshot: &Snapshot, found: TerminalMatch, text: &str) {
    assert_eq!(found.start.line, found.end.line);
    let row = found.start.line + snapshot.display_offset as i32;
    assert!(
        (0..snapshot.rows.len() as i32).contains(&row),
        "{found:?} is off screen at offset {}",
        snapshot.display_offset
    );
    let row = row as usize;
    assert_eq!(snapshot.row_text(row), text);
    assert_eq!(
        snapshot.selection,
        vec![SelectionSpan {
            row,
            start_col: found.start.col,
            end_col: found.end.col + 1,
        }]
    );
}

#[test]
fn next_and_previous() {
    let session = Session::sh(FORTY_LINES, 20, 10);
    session.wait_for_text("line 40");

    // From the cursor at the bottom, forward wraps to the top of history.
    let found = session.find(&TerminalQuery::literal("line 05"), true);
    assert!(found.start.line < 0, "{found:?} should be in history");
    assert_eq!((found.start.col, found.end.col), (0, 6));
    let snapshot = session.terminal.snapshot();
    assert!(snapshot.display_offset > 0);
    assert_selected(&snapshot, found, "line 05");
    let line_05 = found.start.line;

    // Lines 05, 15, 25 and 35 match; each find starts past the selection.
    let fives = TerminalQuery::regex("line [0-9]5");
    for (step, text) in [(10, "line 15"), (20, "line 25"), (30, "line 35")] {
        let found = session.find(&fives, true);
        assert_eq!(found.start, point(line_05 + step, 0));
        assert_selected(&session.terminal.snapshot(), found, text);
    }
    // Line 35 is on screen, so the view is back at the bottom.
    assert_eq!(session.terminal.snapshot().display_offset, 0);

    // Past the last, forward wraps back to the first.
    let found = session.find(&fives, true);
    assert_eq!(found.start, point(line_05, 0));
    assert_selected(&session.terminal.snapshot(), found, "line 05");

    // Backward from the first wraps to the last, then steps back.
    let found = session.find(&fives, false);
    assert_eq!(found.start, point(line_05 + 30, 0));
    assert_selected(&session.terminal.snapshot(), found, "line 35");
    let found = session.find(&fives, false);
    assert_eq!(found.start, point(line_05 + 20, 0));
    let snapshot = session.terminal.snapshot();
    assert!(snapshot.display_offset > 0);
    assert_selected(&snapshot, found, "line 25");
}

#[test]
fn literal_regex_and_case() {
    let session = Session::sh("printf 'axb Foo foo FOO (x)\\n'; read -r x", 40, 5);
    session.wait_for_text("(x)");

    // A literal's metacharacters match only themselves.
    assert_eq!(session.count(&TerminalQuery::literal("a.b")), 0);
    assert_eq!(session.count(&TerminalQuery::regex("a.b")), 1);
    assert_eq!(session.count(&TerminalQuery::literal("(x)")), 1);

    // Smart case: an uppercase letter makes the query case-sensitive.
    let foo = session
        .terminal
        .find_all_visible(&TerminalQuery::literal("Foo"))
        .unwrap();
    assert_eq!(
        foo,
        vec![TerminalMatch {
            start: point(0, 4),
            end: point(0, 6),
        }]
    );
    assert_eq!(session.count(&TerminalQuery::literal("foo")), 3);

    // `case_sensitive` overrides it either way.
    let forced = |text: &str, case_sensitive| TerminalQuery {
        case_sensitive: Some(case_sensitive),
        ..TerminalQuery::literal(text)
    };
    assert_eq!(session.count(&forced("Foo", false)), 3);
    let foo = session
        .terminal
        .find_all_visible(&forced("foo", true))
        .unwrap();
    assert_eq!(
        foo,
        vec![TerminalMatch {
            start: point(0, 8),
            end: point(0, 10),
        }]
    );
    assert_eq!(session.count(&forced("FOO", true)), 1);

    // A regex that doesn't compile is an error, from both calls.
    let broken = TerminalQuery::regex("(");
    assert!(session.terminal.find_all_visible(&broken).is_err());
    let error = session.terminal.find(&broken, true).unwrap_err();
    assert!(!error.0.is_empty());
    assert_eq!(session.terminal.snapshot().selection, Vec::new());

    // An empty query matches nothing.
    assert_eq!(
        session.terminal.find(&TerminalQuery::literal(""), true),
        Ok(None)
    );
    assert_eq!(session.count(&TerminalQuery::literal("")), 0);
}

#[test]
fn a_match_spans_a_soft_wrap() {
    // 12 characters on a 10-column grid: "needle" wraps after "need".
    let session = Session::sh("printf 'xxxxxxneedle\\n'; read -r x", 10, 5);
    session.wait_for_text("le");

    let needle = TerminalQuery::literal("needle");
    let found = session.find(&needle, true);
    assert_eq!(
        found,
        TerminalMatch {
            start: point(0, 6),
            end: point(1, 1),
        }
    );
    assert_eq!(session.terminal.find_all_visible(&needle), Ok(vec![found]));
    assert_eq!(session.terminal.selected_text().as_deref(), Some("needle"));
}

#[test]
fn no_match_and_visible() {
    let session = Session::sh(FORTY_LINES, 20, 10);
    session.wait_for_text("line 40");

    // No match, from the bottom with nothing selected: nothing changes.
    let missing = TerminalQuery::literal("line 99");
    assert_eq!(session.terminal.find(&missing, true), Ok(None));
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.display_offset, 0);
    assert_eq!(snapshot.selection, Vec::new());

    // Nor scrolled back with a match selected.
    session.find(&TerminalQuery::literal("line 05"), true);
    let before = session.terminal.snapshot();
    assert!(before.display_offset > 0);
    assert_eq!(session.terminal.find(&missing, true), Ok(None));
    assert_eq!(session.terminal.find(&missing, false), Ok(None));
    let after = session.terminal.snapshot();
    assert_eq!(after.display_offset, before.display_offset);
    assert_eq!(after.selection, before.selection);

    // At the bottom, only the screen's rows count, not history's.
    session.terminal.scroll_to_bottom();
    let snapshot = session.terminal.snapshot();
    let on_screen = (0..snapshot.rows.len())
        .filter(|&row| snapshot.row_text(row).starts_with("line"))
        .count();
    let lines = TerminalQuery::literal("line");
    let visible = session.terminal.find_all_visible(&lines).unwrap();
    assert_eq!(visible.len(), on_screen);
    assert!(
        visible
            .iter()
            .all(|found| (0..10).contains(&found.start.line))
    );

    // Scrolled back, the visible rows are partly history.
    session.terminal.scroll(-5);
    let visible = session.terminal.find_all_visible(&lines).unwrap();
    assert_eq!(visible.len(), 10);
    let rows: Vec<i32> = visible.iter().map(|found| found.start.line).collect();
    assert_eq!(rows, (-5..5).collect::<Vec<_>>());

    // A screen of 1,200 matches returns the first 1,000.
    let session = Session::sh("printf '%01200d' 0 | tr 0 a; read -r x", 50, 25);
    session.wait_for("1,200 a's", |snapshot| {
        snapshot.text().matches('a').count() == 1200
    });
    let visible = session
        .terminal
        .find_all_visible(&TerminalQuery::literal("a"))
        .unwrap();
    assert_eq!(visible.len(), 1000);
    assert_eq!(visible[999].start, point(19, 49));
}
