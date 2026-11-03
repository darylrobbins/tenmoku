//! Vi copy mode: the vi cursor's motions, selections and searches over a
//! printed fixture with scrollback, and the view holding still as output
//! arrives.

use std::time::{Duration, Instant};

use fe_terminal::{
    CursorShape, GridSize, SelectionKind, Snapshot, Terminal, TerminalEvent, TerminalOptions,
    TerminalQuery, ViMove,
};

const DEADLINE: Duration = Duration::from_secs(5);

const ROWS: u16 = 10;

/// The fixture's last line, left with the shell's cursor after it.
const LAST: &str = "  ab cd:ef  gh.ij (kl) mn";

/// `line 01` to `line {lines}`, then [`LAST`] without a newline; then, at
/// each line read from stdin, 50 more lines.
fn fixture(lines: usize) -> String {
    format!(
        "i=1; while [ $i -le {lines} ]; do printf 'line %02d\\n' $i; i=$((i+1)); done; \
         printf '{LAST}'; \
         while read -r x; do j=1; while [ $j -le 50 ]; do printf '\\nmore %02d' $j; j=$((j+1)); done; done"
    )
}

struct Session {
    terminal: Terminal,
    _events: async_channel::Receiver<TerminalEvent>,
    _dir: tempfile::TempDir,
}

impl Session {
    /// Runs `/bin/sh -c script` on a 40×[`ROWS`] grid keeping `scrollback`
    /// rows of history.
    fn sh(script: &str, scrollback: usize) -> Session {
        Session::sized(script, ROWS, scrollback)
    }

    /// Runs `/bin/sh -c script` on a 40×`rows` grid keeping `scrollback`
    /// rows of history.
    fn sized(script: &str, rows: u16, scrollback: usize) -> Session {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = TerminalOptions {
            cwd: dir.path().to_owned(),
            shell: Some((
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            )),
            scrollback,
            env: Default::default(),
        };
        let size = GridSize {
            cols: 40,
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

    /// The fixture of 30 lines, printed, in vi mode: 21 rows of history.
    fn fixture() -> Session {
        Session::printed(30, TerminalOptions::DEFAULT_SCROLLBACK)
    }

    /// The fixture of `lines` lines, printed, in vi mode.
    fn printed(lines: usize, scrollback: usize) -> Session {
        let session = Session::sh(&fixture(lines), scrollback);
        session.wait_for(LAST, |snapshot| snapshot.row_text(9) == LAST);
        session.terminal.set_vi_mode(true);
        session
    }

    /// Waits up to 5 s for `done`, which is `what`.
    fn wait_for(&self, what: &str, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let start = Instant::now();
        loop {
            let snapshot = self.terminal.snapshot();
            if done(&snapshot) {
                return snapshot;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {what}; screen:\n{}",
                snapshot.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The vi cursor's row and column on screen, and how far back the view
    /// is scrolled.
    fn cursor(&self) -> (usize, usize, usize) {
        let snapshot = self.terminal.snapshot();
        assert!(snapshot.modes.vi);
        assert_eq!(snapshot.cursor.shape, CursorShape::Block);
        (
            snapshot.cursor.row,
            snapshot.cursor.col,
            snapshot.display_offset,
        )
    }

    /// The text of the vi cursor's row, and its column.
    fn at(&self) -> (String, usize) {
        let snapshot = self.terminal.snapshot();
        (snapshot.row_text(snapshot.cursor.row), snapshot.cursor.col)
    }

    fn go(&self, to: ViMove) -> (usize, usize, usize) {
        self.terminal.vi_motion(to);
        self.cursor()
    }

    fn select(&self, kind: SelectionKind) {
        self.terminal.vi_select(Some(kind));
    }

    fn selected(&self) -> Option<String> {
        self.terminal.selected_text()
    }
}

#[test]
fn entering_puts_the_vi_cursor_on_the_shells() {
    let session = Session::sh(&fixture(30), TerminalOptions::DEFAULT_SCROLLBACK);
    session.wait_for(LAST, |snapshot| snapshot.row_text(9) == LAST);
    let shell = session.terminal.snapshot();
    assert!(!shell.modes.vi);
    assert_eq!((shell.cursor.row, shell.cursor.col), (9, LAST.len()));

    session.terminal.set_vi_mode(true);
    assert!(session.terminal.is_vi_mode());
    assert_eq!(session.cursor(), (9, LAST.len(), 0));

    // Scrolled back so the shell's cursor is out of view: the view's
    // top-left cell.
    session.terminal.set_vi_mode(false);
    assert!(!session.terminal.snapshot().modes.vi);
    session.terminal.scroll(-12);
    session.terminal.set_vi_mode(true);
    assert_eq!(session.cursor(), (0, 0, 12));
    assert_eq!(session.at().0, "line 10");
}

#[test]
fn motions_move_as_vim_does() {
    let session = Session::fixture();
    let walk = |to: ViMove, times: usize| -> Vec<(usize, usize)> {
        (0..times)
            .map(|_| {
                let (row, col, _) = session.go(to);
                (row, col)
            })
            .collect()
    };
    let on_last =
        |cols: &[usize]| -> Vec<(usize, usize)> { cols.iter().map(|&c| (9, c)).collect() };

    // `LAST` is "  ab cd:ef  gh.ij (kl) mn": 0, ^ and $.
    assert_eq!(session.go(ViMove::LineStart), (9, 0, 0));
    assert_eq!(session.go(ViMove::FirstNonBlank), (9, 2, 0));
    assert_eq!(session.go(ViMove::LineEnd), (9, LAST.len() - 1, 0));

    // Words stop at punctuation that splits shell words (`:`, brackets);
    // WORDs only at blanks. b goes on to the line above.
    session.go(ViMove::FirstNonBlank);
    assert_eq!(walk(ViMove::WordNext, 6), on_last(&[5, 7, 8, 12, 18, 19]));
    session.go(ViMove::FirstNonBlank);
    assert_eq!(walk(ViMove::WordEnd, 6), on_last(&[3, 6, 7, 9, 16, 18]));
    let mut back = on_last(&[12, 8, 7, 5, 2]);
    back.push((8, 5));
    assert_eq!(walk(ViMove::WordPrevious, 6), back);
    assert_eq!(session.at(), ("line 30".to_owned(), 5));

    session.go(ViMove::Down);
    session.go(ViMove::FirstNonBlank);
    assert_eq!(walk(ViMove::BigWordNext, 3), on_last(&[5, 12, 18]));
    session.go(ViMove::FirstNonBlank);
    assert_eq!(walk(ViMove::BigWordEnd, 4), on_last(&[3, 9, 16, 21]));
    assert_eq!(walk(ViMove::BigWordPrevious, 4), on_last(&[18, 12, 5, 2]));

    // h j k l, and % from a bracket to its match.
    assert_eq!(walk(ViMove::Right, 2), on_last(&[3, 4]));
    assert_eq!(walk(ViMove::Left, 1), on_last(&[3]));
    assert_eq!(walk(ViMove::Up, 2), [(8, 3), (7, 3)]);
    assert_eq!(walk(ViMove::Down, 1), [(8, 3)]);
    session.go(ViMove::Down);
    session.go(ViMove::BigWordNext);
    session.go(ViMove::BigWordNext);
    session.go(ViMove::BigWordNext);
    assert_eq!(session.go(ViMove::Bracket), (9, 21, 0));
    assert_eq!(session.go(ViMove::Bracket), (9, 18, 0));

    // H M L: the view's top, middle and bottom rows, at their first
    // non-blank cells.
    assert_eq!(session.go(ViMove::High), (0, 0, 0));
    assert_eq!(session.at().0, "line 22");
    assert_eq!(session.go(ViMove::Middle), (4, 0, 0));
    assert_eq!(session.go(ViMove::Low), (9, 2, 0));

    // gg: the oldest history line, scrolled to; G: the screen's last line.
    assert_eq!(session.go(ViMove::Top), (0, 0, 21));
    assert_eq!(session.at(), ("line 01".to_owned(), 0));
    assert_eq!(session.go(ViMove::Low), (9, 0, 21));
    assert_eq!(session.at().0, "line 10");
    // Below the view: it follows.
    assert_eq!(session.go(ViMove::Down), (9, 0, 20));
    assert_eq!(session.at().0, "line 11");
    assert_eq!(session.go(ViMove::Bottom), (9, 0, 0));
    assert_eq!(session.at(), (LAST.to_owned(), 0));
}

#[test]
fn vi_scroll_moves_the_view_and_the_cursor_together() {
    let session = Session::fixture();
    session.go(ViMove::Bottom);
    session.go(ViMove::Up);

    // ctrl-u and ctrl-d on a 10-row view: half of it.
    session.terminal.vi_scroll(-5);
    assert_eq!(session.cursor(), (8, 0, 5));
    assert_eq!(session.at().0, "line 25");
    session.terminal.vi_scroll(-5);
    assert_eq!(session.cursor(), (8, 0, 10));
    assert_eq!(session.at().0, "line 20");
    session.terminal.vi_scroll(5);
    assert_eq!(session.cursor(), (8, 0, 5));
    assert_eq!(session.at().0, "line 25");

    // At the bottom the view can't go further: the cursor goes on, to the
    // view's last row.
    session.terminal.vi_scroll(5);
    assert_eq!(session.cursor(), (8, 0, 0));
    session.terminal.vi_scroll(5);
    assert_eq!(session.cursor(), (9, 0, 0));

    // And at the top of history.
    session.go(ViMove::Top);
    session.go(ViMove::Down);
    session.terminal.vi_scroll(-5);
    assert_eq!(session.cursor(), (0, 0, 21));
    assert_eq!(session.at().0, "line 01");
    session.terminal.vi_scroll(10);
    assert_eq!(session.cursor(), (0, 0, 11));
    assert_eq!(session.at().0, "line 11");
}

#[test]
fn selections_follow_the_vi_cursor() {
    let session = Session::fixture();

    // v: the cells from where it started to the cursor.
    session.go(ViMove::FirstNonBlank);
    session.select(SelectionKind::Simple);
    assert_eq!(session.selected().as_deref(), Some("a"));
    session.go(ViMove::WordEnd);
    assert_eq!(session.selected().as_deref(), Some("ab"));
    session.go(ViMove::WordNext);
    assert_eq!(session.selected().as_deref(), Some("ab c"));
    session.go(ViMove::Up);
    // Above where it started: back to it, inclusive.
    assert_eq!(session.selected().as_deref(), Some("30\n  a"));

    // V switches to whole lines, from the same start.
    session.select(SelectionKind::Line);
    assert_eq!(
        session.selected().as_deref(),
        Some(&*format!("line 30\n{LAST}\n"))
    );
    let spans = session.terminal.snapshot().selection;
    assert_eq!(spans.len(), 2);
    assert_eq!((spans[0].row, spans[1].row), (8, 9));
    // V again clears it.
    session.select(SelectionKind::Line);
    assert_eq!(session.selected(), None);

    // ctrl-v: a block.
    session.go(ViMove::High);
    session.select(SelectionKind::Block);
    session.go(ViMove::Down);
    session.go(ViMove::Down);
    session.go(ViMove::Right);
    session.go(ViMove::Right);
    assert_eq!(session.selected().as_deref(), Some("lin\nlin\nlin"));
    session.go(ViMove::LineEnd);
    session.go(ViMove::Left);
    assert_eq!(
        session.selected().as_deref(),
        Some("line 2\nline 2\nline 2")
    );
    session.terminal.vi_select(None);
    assert_eq!(session.selected(), None);
}

#[test]
fn vi_search_moves_to_the_next_and_previous_match() {
    let session = Session::fixture();
    let search = |text: &str, forward: bool| {
        session
            .terminal
            .vi_search(&TerminalQuery::regex(text), forward)
            .expect("compiles")
            .map(|found| (found.start.line, found.start.col))
    };

    // Forward from the bottom wraps to the top of history; the view
    // follows.
    assert_eq!(search(r"line 0[56]", true), Some((-17, 0)));
    assert_eq!(session.cursor(), (0, 0, 17));
    assert_eq!(session.at().0, "line 05");
    assert_eq!(search(r"line 0[56]", true), Some((-16, 0)));
    assert_eq!(session.at().0, "line 06");
    // Strictly after the cursor: the next match, not the one it's on.
    assert_eq!(search(r"line 0[56]", false), Some((-17, 0)));
    // Back from the first wraps to the last.
    assert_eq!(search(r"line 0[56]", false), Some((-16, 0)));
    assert_eq!(session.at().0, "line 06");
    // Smart case, and a match in the middle of a line.
    assert_eq!(search(r"GH", true), None);
    assert_eq!(search(r"gh|kl", true), Some((9, 12)));
    assert_eq!(session.cursor(), (9, 12, 0));
    assert_eq!(search(r"gh|kl", true), Some((9, 19)));
    assert_eq!(search(r"gh|kl", true), Some((9, 12)));
    assert_eq!(search(r"gh|kl", false), Some((9, 19)));

    // No match, or a query that doesn't compile: the cursor stays.
    assert_eq!(search("nowhere", true), None);
    assert_eq!(session.cursor(), (9, 19, 0));
    assert!(
        session
            .terminal
            .vi_search(&TerminalQuery::regex("("), true)
            .is_err()
    );
    assert_eq!(session.cursor(), (9, 19, 0));

    // A selection extends to the match.
    session.go(ViMove::High);
    session.select(SelectionKind::Simple);
    assert_eq!(search("23", true), Some((1, 5)));
    assert_eq!(session.selected().as_deref(), Some("line 22\nline 2"));
}

/// Moves the vi cursor to row 5, column 3, selects to its right, and checks
/// that 50 more lines of output change nothing on screen.
fn assert_output_leaves_the_view(session: &Session) {
    session.go(ViMove::High);
    for _ in 0..5 {
        session.go(ViMove::Down);
    }
    for _ in 0..3 {
        session.go(ViMove::Right);
    }
    session.select(SelectionKind::Simple);
    session.go(ViMove::Right);
    let before = session.terminal.snapshot();
    let at = session.at();
    let selected = session.selected();
    assert_eq!(before.cursor.row, 5);

    // The script prints 50 more lines for each line it reads; the input
    // itself doesn't move the view.
    session.terminal.input_in_place(&b"\r"[..]);
    let after = session.wait_for("50 more lines", |snapshot| {
        snapshot.display_offset >= before.display_offset + 50
    });
    // Let anything still on its way arrive.
    std::thread::sleep(Duration::from_millis(100));
    let after = session
        .terminal
        .snapshot()
        .display_offset
        .max(after.display_offset);
    assert!(after >= before.display_offset + 51, "{after}");
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.rows, before.rows);
    assert_eq!(snapshot.cursor, before.cursor);
    assert_eq!(snapshot.selection, before.selection);
    assert_eq!(session.at(), at);
    assert_eq!(session.selected(), selected);

    // Leaving drops the selection and goes back to the bottom, where the
    // output is.
    session.terminal.set_vi_mode(false);
    let snapshot = session.terminal.snapshot();
    assert!(!snapshot.modes.vi);
    assert_eq!(snapshot.display_offset, 0);
    assert!(snapshot.selection.is_empty());
    assert_eq!(session.selected(), None);
    assert_eq!(snapshot.row_text(9), "more 50");
}

#[test]
fn output_leaves_the_view_where_it_is() {
    let session = Session::fixture();
    assert_output_leaves_the_view(&session);
}

#[test]
fn output_leaves_the_view_where_it_is_with_the_history_full() {
    let session = Session::printed(300, 100);
    assert_eq!(session.terminal.snapshot().history_size, 100);
    assert_output_leaves_the_view(&session);
    assert_eq!(session.terminal.snapshot().history_size, 100);
}

#[test]
fn leaving_scrolls_to_the_bottom_and_drops_the_selection() {
    let session = Session::fixture();
    session.go(ViMove::Top);
    session.select(SelectionKind::Line);
    assert_eq!(session.selected().as_deref(), Some("line 01\n"));
    session.terminal.set_vi_mode(false);
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.display_offset, 0);
    assert_eq!(session.selected(), None);
    // The shell's cursor is back, as the program set it.
    assert_eq!((snapshot.cursor.row, snapshot.cursor.col), (9, LAST.len()));
    // Out of vi mode the vi calls do nothing.
    session.terminal.vi_motion(ViMove::Top);
    session.terminal.vi_scroll(-5);
    assert_eq!(session.terminal.snapshot().display_offset, 0);
}

#[test]
fn vi_prompt_goes_to_the_marked_prompts() {
    // Three marked prompts, each followed by 8 lines, then a marked prompt
    // waiting for input: 18 rows of history.
    let script = r"for i in 1 2 3; do
            printf '\033]133;A\007$ prompt %d\n' $i
            j=1; while [ $j -le 8 ]; do echo out $i $j; j=$((j+1)); done
        done; printf '\033]133;A\007$ '; read x";
    let session = Session::sh(script, TerminalOptions::DEFAULT_SCROLLBACK);
    session.wait_for("out 3 8", |snapshot| snapshot.text().contains("out 3 8"));
    session.terminal.set_vi_mode(true);
    session.terminal.vi_motion(ViMove::Right);
    let prompt = |forward: bool| {
        let moved = session.terminal.vi_prompt(forward);
        (moved, session.at())
    };
    let at = |text: &str| (true, (text.to_owned(), 0));

    assert_eq!(prompt(false), at("$ prompt 3"));
    assert_eq!(prompt(false), at("$ prompt 2"));
    assert_eq!(prompt(false), at("$ prompt 1"));
    assert_eq!(session.cursor(), (0, 0, 18));
    assert!(!session.terminal.vi_prompt(false));
    assert_eq!(prompt(true), at("$ prompt 2"));
    assert_eq!(prompt(true), at("$ prompt 3"));
    assert_eq!(prompt(true), at("$"));
    assert_eq!(session.cursor(), (9, 0, 0));
    assert!(!session.terminal.vi_prompt(true));
}

#[test]
fn switching_kind_keeps_both_ends_selected() {
    let session = Session::fixture();
    // From row 7, column 1 down and left to row 8, column 0: the ends'
    // columns run the other way to their rows.
    session.go(ViMove::High);
    for _ in 0..7 {
        session.go(ViMove::Down);
    }
    session.go(ViMove::Right);
    assert_eq!(session.at(), ("line 29".to_owned(), 1));
    session.select(SelectionKind::Simple);
    session.go(ViMove::Down);
    session.go(ViMove::Left);
    assert_eq!(session.selected().as_deref(), Some("ine 29\nl"));

    session.select(SelectionKind::Block);
    assert_eq!(session.selected().as_deref(), Some("li\nli"));
    session.select(SelectionKind::Simple);
    assert_eq!(session.selected().as_deref(), Some("ine 29\nl"));
    session.select(SelectionKind::Line);
    assert_eq!(session.selected().as_deref(), Some("line 29\nline 30\n"));
    session.select(SelectionKind::Block);
    assert_eq!(session.selected().as_deref(), Some("li\nli"));
    // Adjacent columns, then the same one: motions still move its end.
    session.go(ViMove::Right);
    assert_eq!(session.selected().as_deref(), Some("i\ni"));
    session.select(SelectionKind::Simple);
    assert_eq!(session.selected().as_deref(), Some("ine 29\nli"));
    session.go(ViMove::Right);
    assert_eq!(session.selected().as_deref(), Some("ine 29\nlin"));
}

#[test]
fn the_vi_cursor_stays_in_view_across_a_screen_switch() {
    let script = format!(
        "i=1; while [ $i -le 30 ]; do printf 'line %02d\\n' $i; i=$((i+1)); done; \
         printf '{LAST}'; read x; printf '\\033[?1049halt screen'; read x; \
         printf '\\033[?1049l'; read x"
    );
    let session = Session::sh(&script, TerminalOptions::DEFAULT_SCROLLBACK);
    session.wait_for(LAST, |snapshot| snapshot.row_text(9) == LAST);
    session.terminal.set_vi_mode(true);
    session.go(ViMove::Top);
    for _ in 0..3 {
        session.go(ViMove::Right);
    }
    session.select(SelectionKind::Simple);
    assert_eq!(session.cursor(), (0, 3, 21));

    // The alternate screen has no history: the cursor comes into its view.
    session.terminal.input_in_place(&b"\r"[..]);
    let snapshot = session.wait_for("the alternate screen", |s| s.modes.alt_screen);
    assert!(snapshot.text().contains("alt screen"));
    let (row, _, offset) = session.cursor();
    assert!(row < usize::from(ROWS));
    assert_eq!(offset, 0);
    assert_eq!(session.selected(), None);
    session.go(ViMove::Down);
    session.go(ViMove::LineEnd);
    assert!(session.cursor().0 < usize::from(ROWS));

    // And back on the primary screen.
    session.terminal.input_in_place(&b"\r"[..]);
    session.wait_for("the primary screen", |s| !s.modes.alt_screen);
    let (row, _, _) = session.cursor();
    assert!(row < usize::from(ROWS));
    // The echoed input went into history too.
    let history = session.terminal.snapshot().history_size;
    assert_eq!(session.go(ViMove::Top), (0, 0, history));
    assert_eq!(session.at().0, "line 01");
}

#[test]
fn the_vi_cursor_stays_in_view_as_its_text_leaves_history() {
    let session = Session::printed(300, 100);
    // On the oldest line kept, with a selection to its right.
    session.go(ViMove::Top);
    for _ in 0..3 {
        session.go(ViMove::Right);
    }
    session.select(SelectionKind::Simple);
    session.go(ViMove::Right);
    assert_eq!(session.cursor(), (0, 4, 100));
    let top = session.terminal.snapshot().row_text(0);

    // 51 more rows trim it and the 50 after it.
    session.terminal.input_in_place(&b"\r"[..]);
    session.wait_for("the history to be trimmed", |s| s.row_text(0) != top);
    std::thread::sleep(Duration::from_millis(100));
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.history_size, 100);
    assert_eq!(snapshot.display_offset, 100);
    assert_eq!(session.cursor(), (0, 4, 100));
    assert!(session.selected().is_some());
    session.go(ViMove::Down);
    assert_eq!(session.cursor(), (1, 4, 100));
    session.go(ViMove::Top);
    assert_eq!(session.cursor(), (0, 0, 100));
}

#[test]
fn output_after_leaving_the_alternate_screen_leaves_the_view() {
    // The way back from the alternate screen and 50 lines after it are
    // written at once, so one parser call starts on the alternate screen
    // and scrolls the primary.
    let script = format!(
        "stty -echo; i=1; while [ $i -le 30 ]; do printf 'line %02d\\n' $i; i=$((i+1)); done; \
         printf '{LAST}'; read x; printf '\\033[?1049halt screen'; read x; \
         f=$(mktemp); printf '\\033[?1049l' > \"$f\"; \
         j=1; while [ $j -le 50 ]; do printf '\\nmore %02d' $j >> \"$f\"; j=$((j+1)); done; \
         cat \"$f\"; rm \"$f\"; read x"
    );
    let session = Session::sh(&script, TerminalOptions::DEFAULT_SCROLLBACK);
    let primary = session.wait_for(LAST, |snapshot| snapshot.row_text(9) == LAST);
    session.terminal.set_vi_mode(true);
    session.terminal.input_in_place(&b"\r"[..]);
    session.wait_for("the alternate screen", |s| s.modes.alt_screen);

    session.terminal.input_in_place(&b"\r"[..]);
    let back = session.wait_for("the primary screen and its output", |s| {
        !s.modes.alt_screen && s.history_size >= primary.history_size + 50
    });
    assert_eq!(back.display_offset, 50);
    assert_eq!(back.rows, primary.rows);
    assert_eq!(session.cursor(), (9, LAST.len(), 50));
    session.terminal.set_vi_mode(false);
    assert_eq!(session.terminal.snapshot().row_text(9), "more 50");
}

#[test]
fn middle_of_a_one_row_view_is_its_row() {
    // Without history, then with 5 rows of it.
    for (script, history) in [
        ("printf '  only'; read x", 0),
        (
            "i=1; while [ $i -le 5 ]; do echo line $i; i=$((i+1)); done; printf '  last'; read x",
            5,
        ),
    ] {
        let session = Session::sized(script, 1, TerminalOptions::DEFAULT_SCROLLBACK);
        let shown = session.wait_for("the last line", |s| {
            s.row_text(0).ends_with("only") || s.row_text(0).ends_with("last")
        });
        assert_eq!(shown.history_size, history);
        session.terminal.set_vi_mode(true);
        session.go(ViMove::LineStart);
        assert_eq!(
            session.go(ViMove::Middle),
            (0, 2, 0),
            "{history} rows of history"
        );
        assert_eq!(session.go(ViMove::High), (0, 2, 0));
        assert_eq!(session.go(ViMove::Low), (0, 2, 0));
    }
}

/// The fixture, then the alternate screen at the first line read; at the
/// second, the way back from it and 50 more lines: written at once when
/// `together`, else the 50 lines at a third.
fn alternate_screen(together: bool) -> String {
    alternate_screen_left_by(together, "\\033[?1049l")
}

/// [`alternate_screen`], leaving the alternate screen with `exit`, as
/// `printf` writes it.
fn alternate_screen_left_by(together: bool, exit: &str) -> String {
    let more = "j=1; while [ $j -le 50 ]; do printf '\\nmore %02d' $j; j=$((j+1)); done";
    let back = if together {
        format!("{{ printf '{exit}'; {more}; }} > \"$f\"; cat \"$f\"")
    } else {
        format!("printf '{exit}'; read x; {more} > \"$f\"; cat \"$f\"")
    };
    format!(
        "stty -echo; f=$(mktemp); \
         i=1; while [ $i -le 30 ]; do printf 'line %02d\\n' $i; i=$((i+1)); done; \
         printf '{LAST}'; read x; printf '\\033[?1049halt screen'; read x; {back}; read x"
    )
}

/// A session of [`alternate_screen`] in vi mode, resized to `cols`×`rows`
/// while on the alternate screen.
fn resized_on_the_alternate_screen(together: bool, cols: u16, rows: u16) -> Session {
    on_the_alternate_screen(&alternate_screen(together), Some((cols, rows)))
}

/// A session of `script`, an [`alternate_screen_left_by`], in vi mode,
/// resized to `size` (columns, rows) while on the alternate screen, if any.
fn on_the_alternate_screen(script: &str, size: Option<(u16, u16)>) -> Session {
    let session = Session::sh(script, TerminalOptions::DEFAULT_SCROLLBACK);
    session.wait_for(LAST, |snapshot| snapshot.row_text(9) == LAST);
    session.terminal.set_vi_mode(true);
    session.terminal.input_in_place(&b"\r"[..]);
    session.wait_for("the alternate screen", |s| s.modes.alt_screen);
    if let Some((cols, rows)) = size {
        session.terminal.resize(GridSize {
            cols,
            rows,
            cell_width: 8,
            cell_height: 16,
        });
    }
    session.terminal.input_in_place(&b"\r"[..]);
    session
}

/// The display offset once output has settled, waiting for `done` first.
fn settled(session: &Session, what: &str, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    session.wait_for(what, done);
    std::thread::sleep(Duration::from_millis(100));
    session.terminal.snapshot()
}

#[test]
fn a_resize_on_the_alternate_screen_is_not_output() {
    // Fewer rows, fewer columns (rewrapping the last line), and both.
    for (cols, rows, bottom) in [(40, 5, LAST), (20, 10, "l) mn"), (20, 5, "l) mn")] {
        let size = format!("{cols}x{rows}");
        // Leaving without output: the primary as it reflowed, at the
        // bottom.
        let session = resized_on_the_alternate_screen(false, cols, rows);
        let left = settled(&session, "the primary screen", |s| !s.modes.alt_screen);
        assert_eq!(
            left.display_offset, 0,
            "{size}: the view stays at the bottom"
        );
        assert_eq!(left.rows.len(), usize::from(rows));
        assert_eq!(left.row_text(usize::from(rows) - 1), bottom, "{size}");
        let at = session.at();

        // The output after it leaves the view and the vi cursor on their
        // text.
        session.terminal.input_in_place(&b"\r"[..]);
        let after = settled(&session, "50 more lines", |s| s.display_offset >= 50);
        assert_eq!(after.display_offset, 50, "{size}");
        assert_eq!(after.rows, left.rows, "{size}");
        assert_eq!(after.cursor, left.cursor, "{size}");
        assert_eq!(session.at(), at, "{size}");

        // Output written with the way back holds the same view.
        let together = resized_on_the_alternate_screen(true, cols, rows);
        let held = settled(&together, "the primary screen and its output", |s| {
            !s.modes.alt_screen && s.display_offset >= 50
        });
        assert_eq!(held.display_offset, 50, "{size}");
        assert_eq!(held.rows, left.rows, "{size}");
        assert_eq!(held.cursor, left.cursor, "{size}");
        assert_eq!(together.at(), at, "{size}");
    }
}

#[test]
fn a_long_exit_from_the_alternate_screen_holds_the_view() {
    // More than 32 bytes of parameters, written with the output after it,
    // with and without a resize while the primary was hidden.
    let exit = "\\033[?1;1000;1002;1003;1004;1005;1006;2004;1049l";
    for size in [None, Some((20, 5))] {
        let left = on_the_alternate_screen(&alternate_screen(false), size);
        let left = settled(&left, "the primary screen", |s| !s.modes.alt_screen);
        assert_eq!(left.display_offset, 0, "{size:?}");

        let session = on_the_alternate_screen(&alternate_screen_left_by(true, exit), size);
        let held = settled(&session, "the primary screen and its output", |s| {
            !s.modes.alt_screen && s.display_offset >= 50
        });
        assert_eq!(held.display_offset, 50, "{size:?}");
        assert_eq!(held.rows, left.rows, "{size:?}");
        assert_eq!(held.cursor, left.cursor, "{size:?}");
    }
}

#[test]
fn backward_search_from_a_wide_characters_spacer() {
    let session = Session::sh(
        "printf '\\347\\225\\214 x \\347\\225\\214\\nab'; read x",
        TerminalOptions::DEFAULT_SCROLLBACK,
    );
    session.wait_for("界 x 界", |s| s.text().contains("界 x 界"));
    session.terminal.set_vi_mode(true);
    // Up from column 1 lands on the first 界's spacer, drawn on its first
    // cell.
    session.go(ViMove::LineStart);
    session.go(ViMove::Right);
    assert_eq!(session.go(ViMove::Up), (0, 0, 0));
    let found = session
        .terminal
        .vi_search(&TerminalQuery::literal("界"), false)
        .expect("compiles")
        .map(|found| (found.start.line, found.start.col));
    // Back past the one it's on, wrapping to the last.
    assert_eq!(found, Some((0, 5)));
    assert_eq!(session.cursor(), (0, 5, 0));
}
