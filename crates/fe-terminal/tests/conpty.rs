//! A session on Windows' ConPTY: a program's output reaches the grid, typed
//! input reaches the program, and its exit is reported once.
#![cfg(windows)]

use std::time::{Duration, Instant};

use fe_terminal::{GridSize, Terminal, TerminalEvent, TerminalOptions};

const DEADLINE: Duration = Duration::from_secs(10);

fn spawn(program: &str, args: &[&str]) -> (Terminal, async_channel::Receiver<TerminalEvent>) {
    let options = TerminalOptions {
        cwd: std::env::temp_dir(),
        shell: Some((
            program.to_owned(),
            args.iter().map(|arg| (*arg).to_owned()).collect(),
        )),
        scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
        env: Default::default(),
    };
    let size = GridSize {
        cols: 80,
        rows: 24,
        cell_width: 8,
        cell_height: 16,
    };
    Terminal::spawn(options, size).expect("spawn")
}

/// Waits for `text` on the screen, and returns whether the session exited
/// meanwhile, with its code.
fn wait_for_text(
    terminal: &Terminal,
    events: &async_channel::Receiver<TerminalEvent>,
    text: &str,
) -> Option<Option<i32>> {
    let start = Instant::now();
    let mut exited = None;
    loop {
        while let Ok(event) = events.try_recv() {
            if let TerminalEvent::Exited(code) = event {
                assert!(exited.is_none(), "Exited sent twice");
                exited = Some(code);
            }
        }
        let screen = terminal.snapshot().text();
        if screen.contains(text) {
            return exited;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "timed out waiting for {text:?}; screen:\n{screen}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn output_reaches_the_grid_and_the_exit_is_reported() {
    let (terminal, events) = spawn("cmd.exe", &["/C", "echo hello from conpty"]);
    let mut exited = wait_for_text(&terminal, &events, "hello from conpty");
    let start = Instant::now();
    while exited.is_none() {
        if let Ok(TerminalEvent::Exited(code)) = events.try_recv() {
            exited = Some(code);
        }
        assert!(start.elapsed() < DEADLINE, "no Exited");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(exited, Some(Some(0)));
}

#[test]
fn typed_input_reaches_the_program() {
    let (terminal, events) = spawn("cmd.exe", &["/Q", "/K"]);
    terminal.input(b"echo typed %OS%\r".as_slice());
    wait_for_text(&terminal, &events, "typed Windows_NT");
}
