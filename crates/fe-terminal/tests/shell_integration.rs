//! Shell integration: the working directory from OSC 7 and the prompt marks
//! from OSC 133;A, printed by a `/bin/sh -c` script.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fe_terminal::{GridSize, Snapshot, Terminal, TerminalEvent, TerminalOptions, TerminalQuery};

const DEADLINE: Duration = Duration::from_secs(5);

struct Session {
    terminal: Terminal,
    _events: async_channel::Receiver<TerminalEvent>,
    _dir: tempfile::TempDir,
}

impl Session {
    /// Runs `/bin/sh -c script` on a `cols`×`rows` grid keeping
    /// `scrollback` rows of history.
    fn sh(script: &str, cols: u16, rows: u16, scrollback: usize) -> Session {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = TerminalOptions {
            cwd: dir.path().to_owned(),
            shell: Some((
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            )),
            scrollback,
        };
        let (terminal, events) = Terminal::spawn(options, size(cols, rows)).expect("spawn");
        Session {
            terminal,
            _events: events,
            _dir: dir,
        }
    }

    /// Waits up to 5 s for `text` to show on screen.
    fn wait_for_text(&self, text: &str) -> Snapshot {
        self.wait_for(text, |screen| screen.contains(text))
    }

    /// Waits up to 5 s for `done` to hold of the screen's text, which is
    /// `what`.
    fn wait_for(&self, what: &str, done: impl Fn(&str) -> bool) -> Snapshot {
        let start = Instant::now();
        loop {
            let snapshot = self.terminal.snapshot();
            if done(&snapshot.text()) {
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

    /// Answers the script's `read`, then waits for `text`.
    fn next_step(&self, text: &str) -> Snapshot {
        self.terminal.input(&b"\r"[..]);
        self.wait_for_text(text)
    }

    /// The view's top row.
    fn top(&self) -> String {
        self.terminal.snapshot().row_text(0)
    }

    /// Scrolls back through the prompts until there's none before: the
    /// number of each one landed on, which must be the view's top row.
    fn walk_back(&self) -> Vec<usize> {
        let mut prompts = Vec::new();
        while self.terminal.scroll_to_prompt(false) {
            prompts.push(prompt_number(&self.top()));
        }
        prompts
    }
}

fn size(cols: u16, rows: u16) -> GridSize {
    GridSize {
        cols,
        rows,
        cell_width: 8,
        cell_height: 16,
    }
}

/// `NN` from a row reading `$ prompt NN`.
fn prompt_number(row: &str) -> usize {
    let number = row.strip_prefix("$ prompt ");
    let number = number.unwrap_or_else(|| panic!("{row:?} is not a prompt row"));
    number.trim().parse().unwrap()
}

#[test]
fn cwd_is_the_last_osc_7_directory() {
    let script = r"printf 'ready\n'; read x
        printf '\033]7;file://host/tmp/a%%20b\007one\n'; read x
        printf '\033]7;file://ho'; sleep 0.2; printf 'st/tmp/split\033'; sleep 0.2
        printf '\\two\n'; read x
        printf '\033]7;file:///tmp/%%FF%%2fx\033\\three\n'; read x
        printf '\033]7;/tmp/plain\007\033]7;file:tmp/relative\007\033]7;file://host\007'
        printf '\033]7;file://host/tmp/bad%%zz\007\033]7;file://host/tmp/cut%%2\007four\n'
        read x";
    let session = Session::sh(script, 40, 10, 100);

    session.wait_for_text("ready");
    assert_eq!(session.terminal.cwd(), None);

    // BEL-terminated, percent-decoded.
    session.next_step("one");
    assert_eq!(session.terminal.cwd(), Some(PathBuf::from("/tmp/a b")));

    // ST-terminated, split across writes, ST itself too.
    session.next_step("two");
    assert_eq!(session.terminal.cwd(), Some(PathBuf::from("/tmp/split")));

    // Decoded bytes need not be UTF-8.
    session.next_step("three");
    let path = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff/x"));
    assert_eq!(session.terminal.cwd(), Some(path.clone()));

    // No `file://`, no absolute path, bad escapes: ignored.
    session.next_step("four");
    assert_eq!(session.terminal.cwd(), Some(path));
}

/// Prompts printed, and output lines after each.
const PROMPTS: usize = 40;
const OUTPUT: usize = 10;
const ROWS: u16 = 10;
const SCROLLBACK: usize = 100;

/// Prints [`PROMPTS`] prompts, each marked and followed by [`OUTPUT`] lines
/// of 30 columns; then, after a `read`, prompts 41 and 42 and `CSI 3 J`.
const PROMPTS_SCRIPT: &str = r"i=1; while [ $i -le 40 ]; do
        printf '\033]133;A\007$ prompt %02d\n' $i
        j=1; while [ $j -le 10 ]; do
            printf 'out %02d %02d ....................\n' $i $j; j=$((j+1))
        done; i=$((i+1))
    done; read x
    printf '\033]133;A\007$ prompt 41\nout\n\033]133;A\007$ prompt 42\nout\n\033[3J'
    printf 'cleared'; read x";

#[test]
fn scroll_to_prompt_walks_the_prompts_in_history() {
    let session = Session::sh(PROMPTS_SCRIPT, 40, ROWS, SCROLLBACK);
    session.wait_for_text("out 40 10");
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.history_size, SCROLLBACK);
    assert_eq!(snapshot.display_offset, 0);

    // Prompt n is printed on row (n - 1) × 11 + 1, and the cursor waits on
    // the row after the last output: the grid holds the last 110 rows.
    let block = 1 + OUTPUT;
    let rows = PROMPTS * block + 1;
    let first = rows - (SCROLLBACK + usize::from(ROWS)) + 1;
    let kept: Vec<usize> = (1..=PROMPTS)
        .filter(|n| (n - 1) * block + 1 >= first)
        .collect();
    assert!(kept.len() > 5, "{kept:?}");

    // Back from the bottom: every prompt in history, newest first, each at
    // the top of the view.
    let mut newest_first = kept.clone();
    newest_first.reverse();
    assert_eq!(session.walk_back(), newest_first);
    // There's none before the oldest: the view stays.
    assert_eq!(prompt_number(&session.top()), kept[0]);

    // Forward to the newest, then to the bottom.
    for &n in &kept[1..] {
        assert!(session.terminal.scroll_to_prompt(true));
        assert_eq!(prompt_number(&session.top()), n);
    }
    assert!(!session.terminal.scroll_to_prompt(true));
    assert_eq!(session.terminal.snapshot().display_offset, 0);

    // Half the columns: each output line wraps onto a second row, and the
    // history's top rows go. The marks follow their lines.
    session.terminal.resize(size(20, ROWS));
    assert_eq!(session.terminal.snapshot().history_size, SCROLLBACK);
    let rewrapped = session.walk_back();
    assert!(rewrapped.len() >= 3, "{rewrapped:?}");
    let expected: Vec<usize> = (0..rewrapped.len()).map(|ix| PROMPTS - ix).collect();
    assert_eq!(rewrapped, expected);
    // The oldest is the first prompt in history: its predecessor's row went.
    session.terminal.scroll(-(SCROLLBACK as i32));
    let history = session.terminal.snapshot().text();
    let oldest = rewrapped.last().unwrap();
    assert!(!history.contains(&format!("$ prompt {:02}", oldest - 1)));
    assert!(history.contains(&format!("out {:02} ", oldest - 1)));

    // Typing goes back to the bottom; then two more prompts and `CSI 3 J`
    // clears history: only the marks on screen are left.
    session.next_step("cleared");
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.history_size, 0);
    assert!(snapshot.text().contains("$ prompt 41"));
    assert!(session.terminal.scroll_to_prompt(false));
    assert!(session.terminal.scroll_to_prompt(false));
    assert!(!session.terminal.scroll_to_prompt(false));
    assert_eq!(session.terminal.snapshot().display_offset, 0);
}

#[test]
fn without_marks_scroll_to_prompt_does_nothing() {
    let session = Session::sh(
        "i=1; while [ $i -le 30 ]; do echo line $i; i=$((i+1)); done; read x",
        40,
        ROWS,
        SCROLLBACK,
    );
    session.wait_for_text("line 30");
    session.terminal.scroll(-5);
    assert!(!session.terminal.scroll_to_prompt(false));
    assert_eq!(session.terminal.snapshot().display_offset, 5);
    assert!(!session.terminal.scroll_to_prompt(true));
    assert_eq!(session.terminal.snapshot().display_offset, 5);
}

/// A shell loop printing prompts `from` to `to`, each marked and followed by
/// `lines` output lines of 30 columns.
fn prompts(from: usize, to: usize, lines: usize) -> String {
    format!(
        r"i={from}; while [ $i -le {to} ]; do
            printf '\033]133;A\007$ prompt %02d\n' $i
            j=1; while [ $j -le {lines} ]; do
                printf 'out %02d %02d ....................\n' $i $j; j=$((j+1))
            done; i=$((i+1))
        done"
    )
}

/// Asserts `walked` counts down from `newest`, more than `at_least` long.
fn assert_counts_down(walked: &[usize], newest: usize, at_least: usize) {
    assert!(walked.len() > at_least, "{walked:?}");
    let expected: Vec<usize> = (0..walked.len()).map(|ix| newest - ix).collect();
    assert_eq!(walked, expected);
}

#[test]
fn a_synchronized_update_that_clears_and_refills_history() {
    // The update is written to a file first, so `cat` prints it at once.
    let script = format!(
        r"{}
        {{ printf '\033[?2026h\033[3J'; {}; printf '\033[?2026l'; }} > update
        cat update; printf 'done'; read x",
        prompts(1, 20, OUTPUT),
        prompts(21, 40, OUTPUT),
    );
    let session = Session::sh(&script, 40, ROWS, SCROLLBACK);
    session.wait_for_text("done");
    assert_eq!(session.terminal.snapshot().history_size, SCROLLBACK);
    // Every prompt landed on is one the update printed.
    let walked = session.walk_back();
    assert_counts_down(&walked, 40, 5);
    assert!(*walked.last().unwrap() > 20, "{walked:?}");
}

#[test]
fn a_synchronized_update_longer_than_the_headroom() {
    // 2,200 rows in one update: twice the history's headroom.
    let script = format!(
        r"{{ printf '\033[?2026h'; {}; printf '\033[?2026l'; }} > update
        cat update; printf 'done'; read x",
        prompts(1, 200, OUTPUT),
    );
    let session = Session::sh(&script, 40, ROWS, SCROLLBACK);
    session.wait_for_text("done");
    assert_eq!(session.terminal.snapshot().history_size, SCROLLBACK);
    assert_counts_down(&session.walk_back(), 200, 5);
}

#[test]
fn marks_survive_fewer_rows_and_columns_at_once() {
    // Two prompts at the top of a 40×10 grid; the rows below the cursor are
    // blank.
    let script = format!("{}; read x", prompts(1, 2, 1));
    let session = Session::sh(&script, 40, ROWS, SCROLLBACK);
    session.wait_for_text("out 02 01");

    // 20×5 drops blank rows at the bottom and wraps the output lines.
    session.terminal.resize(size(20, 5));
    assert!(session.terminal.scroll_to_prompt(false));
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.top(), "$ prompt 01");
    assert!(!session.terminal.scroll_to_prompt(false));
}

#[test]
fn marks_follow_a_reflow_on_the_alternate_screen() {
    let script = format!(
        r"{}; read x
        printf '\033[?1049hon the alternate screen'; read x
        printf '\033[?1049lback'; read x",
        prompts(1, 20, 5),
    );
    let session = Session::sh(&script, 40, ROWS, SCROLLBACK);
    session.wait_for_text("out 20 05");
    session.next_step("on the alternate screen");

    // Half the columns while the primary grid is hidden: its output wraps.
    session.terminal.resize(size(20, ROWS));
    session.next_step("back");
    assert_counts_down(&session.walk_back(), 20, 3);
}

#[test]
fn the_view_returning_to_the_bottom_forgets_the_prompt() {
    let session = Session::sh(PROMPTS_SCRIPT, 40, ROWS, SCROLLBACK);
    session.wait_for_text("out 40 10");
    let back = |n: usize| {
        assert!(session.terminal.scroll_to_prompt(false));
        assert_eq!(prompt_number(&session.top()), n);
    };
    back(40);
    back(39);

    // Scrolled to the bottom: back starts from the shell's prompt again.
    session.terminal.scroll(1_000);
    back(40);
    back(39);

    // A match on screen at the bottom.
    let query = TerminalQuery::literal("out 40 10");
    session.terminal.find(&query, false).unwrap().unwrap();
    assert_eq!(session.terminal.snapshot().display_offset, 0);
    back(40);

    // A taller grid that shows the whole of prompt 40: back lands on it, at
    // the bottom, rather than going on to 39.
    session.terminal.resize(size(40, 2 * ROWS));
    assert_eq!(session.terminal.snapshot().display_offset, 0);
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.terminal.snapshot().display_offset, 0);
    back(39);
}

/// Fills a default-sized history with 1,000 prompts, then narrows the grid
/// so the output lines wrap and the history overflows: on the alternate
/// screen, if `hidden`. The prompts still in history are each landed on.
fn narrow_a_full_history(hidden: bool) {
    let script = format!(
        r"{{ {}; }} > out; cat out; printf 'ready'; read x
        printf '\033[?1049halt'; read x
        printf '\033[?1049lback'; read x",
        prompts(1, 1000, OUTPUT),
    );
    let scrollback = TerminalOptions::DEFAULT_SCROLLBACK;
    let session = Session::sh(&script, 40, ROWS, scrollback);
    session.wait_for_text("ready");
    assert_eq!(session.terminal.snapshot().history_size, scrollback);
    if hidden {
        session.next_step("alt");
    }
    // Each output line takes two rows: twice the history, and more than
    // the headroom over.
    session.terminal.resize(size(16, ROWS));
    if hidden {
        session.next_step("back");
    }
    assert_eq!(session.terminal.snapshot().history_size, scrollback);
    assert_counts_down(&session.walk_back(), 1000, 100);
}

#[test]
fn marks_survive_narrowing_a_full_history() {
    narrow_a_full_history(false);
}

#[test]
fn marks_survive_narrowing_a_full_history_on_the_alternate_screen() {
    narrow_a_full_history(true);
}

/// Prints the prompts, then `ready`, and after a pause, without input,
/// switches to the alternate screen; `then` follows.
fn alternate_after_a_pause(then: &str) -> String {
    format!(
        r"{}; printf 'ready'; sleep 1; printf '\033[?1049halt'; {then}",
        prompts(1, PROMPTS, OUTPUT),
    )
}

#[test]
fn a_hidden_resize_that_puts_the_view_at_the_bottom_forgets_the_prompt() {
    let script = alternate_after_a_pause(r"sleep 1; printf '\033[?1049lback'; read x");
    let session = Session::sh(&script, 40, ROWS, SCROLLBACK);
    session.wait_for_text("ready");
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.top(), "$ prompt 40");
    assert!(session.terminal.snapshot().display_offset > 0);

    // Taller while hidden: prompt 40 is on screen once the primary shows.
    session.wait_for_text("alt");
    session.terminal.resize(size(40, 2 * ROWS));
    session.wait_for_text("back");
    assert_eq!(session.terminal.snapshot().display_offset, 0);
    // Back starts from the shell's prompt, not from 40.
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.terminal.snapshot().display_offset, 0);
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.top(), "$ prompt 39");
}

#[test]
fn typing_on_the_alternate_screen_forgets_the_prompt() {
    let script = alternate_after_a_pause(r"read x; printf '\033[?1049l'; read x");
    let session = Session::sh(&script, 40, ROWS, SCROLLBACK);
    session.wait_for_text("ready");
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.top(), "$ prompt 40");

    // Narrower while hidden, so the marks are placed again as it shows;
    // typing there forgot prompt 40 first. Typing scrolls the alternate
    // screen, so the primary's view shows history again.
    session.wait_for_text("alt");
    session.terminal.resize(size(25, ROWS));
    session.terminal.input(&b"\r"[..]);
    session.wait_for("the primary grid", |screen| {
        !screen.contains("alt") && screen.contains("out 40")
    });
    assert!(session.terminal.scroll_to_prompt(false));
    assert_eq!(session.top(), "$ prompt 40");
}
