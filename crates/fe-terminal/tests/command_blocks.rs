// Drives `/bin/sh -c` scripts that print OSC 133 marks.
#![cfg(unix)]

//! Command blocks: where each command's prompt, command line, output and
//! end are, from OSC 133 A, B, C and D, and how it ended.

use std::time::{Duration, Instant};

use fe_terminal::{CommandBlock, GridSize, Snapshot, Terminal, TerminalEvent, TerminalOptions};

const DEADLINE: Duration = Duration::from_secs(10);

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
            env: Default::default(),
        };
        let (terminal, events) = Terminal::spawn(options, size(cols, rows)).expect("spawn");
        Session {
            terminal,
            _events: events,
            _dir: dir,
        }
    }

    /// Waits for `text` to show on screen.
    fn wait_for_text(&self, text: &str) -> Snapshot {
        let start = Instant::now();
        loop {
            let snapshot = self.terminal.snapshot();
            if snapshot.text().contains(text) {
                return snapshot;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {text:?}; screen:\n{}",
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

    /// The blocks in view, once `done` holds of them.
    fn wait_for_blocks(
        &self,
        what: &str,
        done: impl Fn(&[CommandBlock]) -> bool,
    ) -> Vec<CommandBlock> {
        let start = Instant::now();
        loop {
            let blocks = self.terminal.command_blocks();
            if done(&blocks) {
                return blocks;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {what}; blocks: {blocks:#?}\nscreen:\n{}",
                self.terminal.snapshot().text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
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

/// `printf` arguments for OSC 133 marks.
const A: &str = r"\033]133;A\007";
const B: &str = r"\033]133;B\007";
const C: &str = r"\033]133;C\007";

fn d(code: i32) -> String {
    format!(r"\033]133;D;{code}\007")
}

/// A block's rows, exit code, and whether it finished.
fn rows(
    block: &CommandBlock,
) -> (
    isize,
    Option<isize>,
    Option<isize>,
    Option<isize>,
    Option<i32>,
) {
    (
        block.prompt_row,
        block.input_row,
        block.output_start_row,
        block.end_row,
        block.exit_code,
    )
}

#[test]
fn blocks_record_rows_and_exit_codes() {
    let script = format!(
        r"printf '{A}$ {B}echo one\n{C}one\n{d0}'
        printf '{A}$ {B}false\n{C}{d1}'
        printf '{A}$ {B}exit 7\n{C}gone\n{d7}'
        printf '{A}$ {B}'; read x",
        d0 = d(0),
        d1 = d(1),
        d7 = d(7),
    );
    let session = Session::sh(&script, 40, 10, 100);
    let blocks = session.wait_for_blocks("four blocks", |blocks| blocks.len() == 4);
    let screen = session.terminal.snapshot();
    assert_eq!(screen.row_text(0), "$ echo one");
    assert_eq!(screen.row_text(2), "$ false");
    assert_eq!(screen.row_text(3), "$ exit 7");
    assert_eq!(screen.row_text(5), "$");

    // `echo one`: its output is row 1, and `D` on row 2 ends it there.
    assert_eq!(rows(&blocks[0]), (0, Some(0), Some(1), Some(1), Some(0)));
    // `false` printed nothing: its block is its prompt's row, and its
    // output would have started on the row after.
    assert_eq!(rows(&blocks[1]), (2, Some(2), Some(3), Some(2), Some(1)));
    assert_eq!(rows(&blocks[2]), (3, Some(3), Some(4), Some(4), Some(7)));
    // The prompt waiting for a command: no output, no end.
    assert_eq!(rows(&blocks[3]), (5, Some(5), None, None, None));
    for block in &blocks[..3] {
        assert!(!block.running());
        assert!(block.finished.is_some());
        assert!(block.duration().is_some());
    }
    assert!(!blocks[3].running());
    assert_eq!(blocks[3].finished, None);
    assert_eq!(blocks[3].duration(), None);

    // Ids follow the order the shell printed them in.
    let ids: Vec<u64> = blocks.iter().map(|block| block.id).collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");

    // The snapshot taken with them has the same rows.
    let (snapshot, with) = session.terminal.snapshot_with_blocks();
    assert_eq!(with, blocks);
    assert_eq!(snapshot.row_text(blocks[2].prompt_row as usize), "$ exit 7");
}

#[test]
fn a_running_block_has_no_end() {
    let script = format!(
        r"printf '{A}$ {B}make\n{C}working\n'; read x; sleep 0.2
        printf 'built\n{d0}{A}$ {B}'; read x",
        d0 = d(0),
    );
    let session = Session::sh(&script, 40, 10, 100);
    session.wait_for_text("working");
    let blocks = session.wait_for_blocks("the running block", |blocks| blocks.len() == 1);
    let block = &blocks[0];
    assert!(block.running());
    assert_eq!(rows(block), (0, Some(0), Some(1), None, None));
    assert_eq!(block.finished, None);
    assert_eq!(block.duration(), None);
    // Its output so far.
    assert_eq!(
        session.terminal.command_output(block.id).as_deref(),
        Some("working")
    );

    // It ends: the row before the next prompt is its last, and it ran for
    // at least the 0.2 s it slept.
    session.next_step("built");
    let blocks = session.wait_for_blocks("the next prompt", |blocks| blocks.len() == 2);
    let ended = &blocks[0];
    assert!(!ended.running());
    assert_eq!(rows(ended), (0, Some(0), Some(1), Some(3), Some(0)));
    assert!(ended.duration().unwrap() >= Duration::from_millis(200));
    assert_eq!(
        session.terminal.command_output(ended.id).as_deref(),
        Some("working\n\nbuilt")
    );
}

#[test]
#[allow(non_snake_case)]
fn a_missing_D_leaves_no_exit_code() {
    // A shell that sends no `D`: the next prompt ends the block. Then
    // bash's `D` for an empty command line, which never ran (no `C`).
    let script = format!(
        r"printf '{A}$ {B}ls\n{C}a b\n'
        printf '{A}$ {B}\n{d0}'
        printf '{A}$ {B}'; read x",
        d0 = d(0),
    );
    let session = Session::sh(&script, 40, 10, 100);
    let blocks = session.wait_for_blocks("three blocks", |blocks| blocks.len() == 3);
    assert_eq!(rows(&blocks[0]), (0, Some(0), Some(1), Some(1), None));
    assert!(!blocks[0].running());
    assert!(blocks[0].finished.is_some());
    assert!(blocks[0].duration().is_some());

    assert_eq!(rows(&blocks[1]), (2, Some(2), None, Some(2), None));
    assert!(blocks[1].finished.is_some());
    assert_eq!(blocks[1].duration(), None);
    assert_eq!(session.terminal.command_output(blocks[1].id), None);
}

/// The output lines each block of [`blocks_script`] prints.
const OUTPUT: usize = 10;

/// Blocks `from` to `to`, each a marked prompt `$ prompt NN` and
/// [`OUTPUT`] lines of 30 columns, ending with exit code `NN % 4`.
fn blocks_script(from: usize, to: usize) -> String {
    format!(
        r"i={from}; while [ $i -le {to} ]; do
            printf '{A}$ prompt %02d{B}\n{C}' $i
            j=1; while [ $j -le {OUTPUT} ]; do
                printf 'out %02d %02d ....................\n' $i $j; j=$((j+1))
            done
            printf '\033]133;D;%d\007' $((i % 4)); i=$((i+1))
        done"
    )
}

/// `NN` from a row reading `$ prompt NN`.
fn prompt_number(row: &str) -> Option<usize> {
    row.strip_prefix("$ prompt ")?.get(..2)?.parse().ok()
}

/// Pages through history from the top to the bottom, checking each block
/// in view against the screen: its prompt row reads `$ prompt NN`, its
/// output starts with `out NN 01`, it ends with `NN % 4` on the row before
/// the next block's prompt. The numbers of the blocks whose prompts were
/// seen.
fn check_blocks_through_history(session: &Session) -> Vec<usize> {
    let terminal = &session.terminal;
    terminal.scroll(-1_000_000);
    let mut seen = Vec::new();
    loop {
        let (snapshot, blocks) = terminal.snapshot_with_blocks();
        let rows = snapshot.rows.len() as isize;
        let text = |row: isize| {
            (0..rows)
                .contains(&row)
                .then(|| snapshot.row_text(row as usize))
        };
        for pair in blocks.windows(2) {
            assert_eq!(pair[0].end_row, Some(pair[1].prompt_row - 1), "{pair:#?}");
        }
        for block in &blocks {
            let Some(prompt) = text(block.prompt_row) else {
                continue;
            };
            let Some(n) = prompt_number(&prompt) else {
                panic!(
                    "{prompt:?} is not a prompt; {block:#?}\n{}",
                    snapshot.text()
                );
            };
            if n == 41 {
                assert_eq!(block.end_row, None);
                continue;
            }
            assert_eq!(block.exit_code, Some((n % 4) as i32), "prompt {n}");
            if let Some(output) = text(block.output_start_row.unwrap()) {
                assert!(output.starts_with(&format!("out {n:02} 01")), "{output:?}");
            }
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
        if snapshot.display_offset == 0 {
            return seen;
        }
        terminal.scroll(snapshot.rows.len() as i32);
    }
}

#[test]
fn blocks_survive_history_trimming_and_the_alternate_screen() {
    let script = format!(
        r"{}; printf '{A}$ prompt 41{B}'; read x
        printf '\033[?1049halt'; read x
        printf '\033[?1049lback'; read x",
        blocks_script(1, 40),
    );
    let session = Session::sh(&script, 40, 10, 100);
    session.wait_for_text("$ prompt 41");
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.history_size, 100);

    // Block n's prompt is row (n - 1) × 11; the grid keeps the last 110 of
    // 441 rows, from row 331, which is block 31's first output row.
    let blocks = session.terminal.command_blocks();
    assert_eq!(blocks.len(), 2, "{blocks:#?}");
    assert_eq!(rows(&blocks[0]), (-2, Some(-2), Some(-1), Some(8), Some(0)));
    assert_eq!(rows(&blocks[1]), (9, Some(9), None, None, None));

    // At the top of history: block 31, its prompt's row trimmed away.
    session.terminal.scroll(-100);
    let blocks = session.terminal.command_blocks();
    assert_eq!(blocks.len(), 1, "{blocks:#?}");
    assert_eq!(rows(&blocks[0]), (-1, Some(-1), Some(0), Some(9), Some(3)));
    let output = session.terminal.command_output(blocks[0].id).unwrap();
    assert_eq!(output.lines().count(), OUTPUT);
    assert!(output.starts_with("out 31 01"), "{output:?}");
    assert_eq!(
        check_blocks_through_history(&session),
        (32..=40).collect::<Vec<_>>()
    );

    // None on the alternate screen; then half the columns while it hides
    // the primary grid, whose output lines wrap onto two rows each.
    session.next_step("alt");
    assert_eq!(session.terminal.command_blocks(), Vec::new());
    session.terminal.resize(size(20, 10));
    session.next_step("back");
    assert_eq!(session.terminal.snapshot().history_size, 100);
    let seen = check_blocks_through_history(&session);
    assert!(seen.len() >= 3, "{seen:?}");
    assert_eq!(*seen.last().unwrap(), 40, "{seen:?}");
    let blocks = session.terminal.command_blocks();
    let last = blocks.last().unwrap();
    assert_eq!(last.end_row, None);
    assert_eq!(
        session
            .terminal
            .snapshot()
            .row_text(last.prompt_row as usize),
        "$ prompt 41"
    );
}

#[test]
fn the_cap_of_1000_drops_the_oldest() {
    // 1,005 one-row blocks and a prompt: 1,006, all of whose rows the
    // history keeps.
    let script = format!(
        r"i=0; while [ $i -lt 1005 ]; do
            printf '{A}$ {B}{C}%d\n{d0}' $i; i=$((i+1))
        done > out; cat out; printf '{A}$ {B}done'; read x",
        d0 = d(0),
    );
    let session = Session::sh(&script, 40, 10, TerminalOptions::DEFAULT_SCROLLBACK);
    session.wait_for_text("done");
    let blocks = session.wait_for_blocks("the last prompt", |blocks| {
        blocks.last().is_some_and(|block| block.id == 1005)
    });
    assert_eq!(blocks.last().unwrap().prompt_row, 9);

    // Block n is row n: at the top, the first six have gone.
    session.terminal.scroll(-1_000_000);
    let snapshot = session.terminal.snapshot();
    assert_eq!(snapshot.row_text(0), "$ 0");
    let blocks = session.terminal.command_blocks();
    let ids: Vec<u64> = blocks.iter().map(|block| block.id).collect();
    assert_eq!(ids, vec![6, 7, 8, 9]);
    for block in &blocks {
        assert_eq!(block.prompt_row, block.id as isize);
        assert_eq!(block.end_row, Some(block.prompt_row));
    }
    assert_eq!(session.terminal.command_output(5), None);
    assert_eq!(session.terminal.command_output(6).as_deref(), Some("6"));
}

#[test]
fn command_and_output_text_are_readable() {
    // A command line that wraps onto a second row, output with no line
    // break at its end, and a command line still being typed.
    let script = format!(
        r"printf '{A}$ {B}echo hello world && echo with a long line\n{C}hello world\nwith a long line\n{d0}'
        printf '{A}$ {B}printf partial\n{C}partial{d0}\n'
        printf '{A}$ {B}typing'; read x
        printf '\033[?1049halt'; read x",
        d0 = d(0),
    );
    let session = Session::sh(&script, 30, 10, 100);
    let blocks = session.wait_for_blocks("three blocks", |blocks| blocks.len() == 3);
    let terminal = &session.terminal;

    let first = &blocks[0];
    assert_eq!(rows(first), (0, Some(0), Some(2), Some(3), Some(0)));
    assert_eq!(
        terminal.command_text(first.id).as_deref(),
        Some("echo hello world && echo with a long line")
    );
    assert_eq!(
        terminal.command_output(first.id).as_deref(),
        Some("hello world\nwith a long line")
    );

    // `D` mid-row: the row it's on is the block's last.
    let second = &blocks[1];
    assert_eq!(rows(second), (4, Some(4), Some(5), Some(5), Some(0)));
    assert_eq!(
        terminal.command_text(second.id).as_deref(),
        Some("printf partial")
    );
    assert_eq!(
        terminal.command_output(second.id).as_deref(),
        Some("partial")
    );

    // Not run yet: no command line, no output.
    let third = &blocks[2];
    assert_eq!(terminal.command_text(third.id), None);
    assert_eq!(terminal.command_output(third.id), None);
    // An id the session never had.
    assert_eq!(terminal.command_text(99), None);

    // The primary grid's text is out of reach on the alternate screen.
    session.next_step("alt");
    assert_eq!(terminal.command_text(first.id), None);
    assert_eq!(terminal.command_output(first.id), None);
}
