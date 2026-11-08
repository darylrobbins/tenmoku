//! The terminal on the bottom dock with a session of fixed output
//! (`TerminalPanel::open_with_session`): two finished commands, one that
//! passed (exit 0) and one that failed (exit 1), and one still running, each
//! marked by OSC 133 as a shell integration marks them, with ANSI colours.
//! No user shell, profile or setting is in it.

use std::time::{Duration, Instant};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "terminal_session",
    steps: |_| vec![open(), run("terminal.toggle"), wait(), idle()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The terminal, right of its output.
    probe(600., 500., "surface.background"),
    // The panel's header.
    probe(600., 351., "surface.panel"),
];

/// The last text the session writes: once it shows, all of it has.
const LAST: &str = "Listening on http://127.0.0.1:8080";

/// What the session writes: a prompt, the command typed, its output and its
/// exit code for each command, by OSC 133's A, B, C and D marks.
fn session() -> String {
    let prompt = "\x1b]133;A\x07\x1b[1;34m~/demo\x1b[0m \x1b[32m$\x1b[0m \x1b]133;B\x07";
    let output = "\x1b]133;C\x07";
    let done = |code: i32| format!("\x1b]133;D;{code}\x07");
    [
        prompt.to_owned(),
        "cargo build\r\n".to_owned(),
        output.to_owned(),
        "   \x1b[1;32mCompiling\x1b[0m demo v0.1.0 (~/demo)\r\n".to_owned(),
        "    \x1b[1;32mFinished\x1b[0m `dev` profile [unoptimized + debuginfo] target(s) \
         in 0.42s\r\n"
            .to_owned(),
        done(0),
        prompt.to_owned(),
        "cargo test\r\n".to_owned(),
        output.to_owned(),
        "test counts_words ... \x1b[32mok\x1b[0m\r\n".to_owned(),
        "test counts_lines ... \x1b[31mFAILED\x1b[0m\r\n".to_owned(),
        "\x1b[1;31merror\x1b[0m: test failed, to rerun pass `--bin demo`\r\n".to_owned(),
        done(1),
        prompt.to_owned(),
        "cargo run -- serve\r\n".to_owned(),
        output.to_owned(),
        format!("{LAST}\r\n"),
    ]
    .concat()
}

/// Opens the session in the terminal panel: its process starts when the
/// panel next lays it out.
fn open() -> Step {
    Box::new(|workbench, window, cx| {
        let bytes = session();
        (workbench.terminal().clone())
            .update(cx, |panel, cx| panel.open_with_session(&bytes, window, cx));
    })
}

/// Waits, up to 10 s, for the session's process to have written it all:
/// it runs beside the app, so no frame or task waits for it. Fails the
/// scene if it hasn't by then, or there's no session.
fn wait() -> Step {
    Box::new(|workbench, _, cx| {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let panel = workbench.terminal().read(cx);
            let terminal = panel.view().and_then(|view| view.read(cx).terminal());
            if terminal.is_some_and(|terminal| terminal.output_text().contains(LAST)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fail("the terminal session didn't write its output in 10 s".to_owned());
    })
}
