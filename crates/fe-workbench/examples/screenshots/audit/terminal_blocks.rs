//! Terminal command blocks (T1595), in the terminal on the bottom dock with
//! a session of fixed output (`TerminalPanel::open_with_session`): a command
//! that passed (exit 0) and one that failed (exit 1), each marked by OSC 133
//! as a shell integration marks them, then the live prompt. The passing
//! block has a celadon edge down the grid's left edge, under the text, and
//! no label (it ran under a second); the failing one a shinsha edge and
//! `exit 1 · 0.0 s` at the right of its prompt row. No user shell, profile
//! or setting is in it.

use std::time::{Duration, Instant};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "terminal_blocks",
    steps: |_| vec![open(), run("terminal.toggle"), wait(), drawn(), idle()],
    probes: PROBES,
    ..DEFAULT
};

// Rows: 0 `cargo build`'s prompt, 1 its output; 2 `cargo test`'s prompt,
// 3 blank, 4 to 6 its output; 7 the live prompt; 8 empty. The edges are
// under the text, so the points are on rows whose first cell is blank, or
// a row's top pixel, above any glyph's ink.
const PROBES: &[Probe] = &[
    // The passing block's edge: its prompt row's top, its output row.
    probe(EDGE_X, TOP + 1., "status.success"),
    probe(EDGE_X, TOP + 1.5 * ROW, "status.success"),
    // The failing block's edge: its prompt row's top, the blank row.
    probe(EDGE_X, TOP + 2. * ROW + 1., "status.error"),
    probe(EDGE_X, TOP + 3.5 * ROW, "status.error"),
    // Right of the edge in the first cell of a blank-led row, and the empty
    // row below the live prompt: the terminal's ground.
    probe(EDGE_X + 4., TOP + 1.5 * ROW, "surface.background"),
    probe(EDGE_X, TOP + 8.5 * ROW, "surface.background"),
];

/// The middle of the edge, 2 px wide at the grid's left: the centre starts
/// at x = 282, right of the 240 px sidebar and the rail.
const EDGE_X: f32 = 283.;
/// The top of the session's first row, under the dock's strip, and a row's
/// height (Code small).
const TOP: f32 = 361.;
const ROW: f32 = 18.;

/// The last text the session writes: once it shows, all of it has.
const LAST: &str = "~/demo $";

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
        "    \x1b[1;32mFinished\x1b[0m `dev` profile [unoptimized + debuginfo] target(s) \
         in 0.42s\r\n"
            .to_owned(),
        done(0),
        prompt.to_owned(),
        "cargo test\r\n".to_owned(),
        output.to_owned(),
        "\r\nrunning 2 tests\r\n".to_owned(),
        "test counts_words ... \x1b[32mok\x1b[0m\r\n".to_owned(),
        "test counts_lines ... \x1b[31mFAILED\x1b[0m\r\n".to_owned(),
        done(1),
        "\x1b]133;A\x07\x1b[1;34m~/demo\x1b[0m \x1b[32m$\x1b[0m ".to_owned(),
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

/// Waits, up to 10 s, for the session's process to have written it all and
/// both blocks to have ended: it runs beside the app, so no frame or task
/// waits for it. Fails the scene if it hasn't by then, or there's no
/// session.
fn wait() -> Step {
    Box::new(|workbench, _, cx| {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let panel = workbench.terminal().read(cx);
            let terminal = panel.view().and_then(|view| view.read(cx).terminal());
            let ended = terminal.is_some_and(|terminal| {
                terminal.output_text().contains(LAST)
                    && (terminal.command_blocks().iter())
                        .filter(|block| block.exit_code.is_some())
                        .count()
                        == 2
            });
            if ended {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fail("the terminal session didn't write its blocks in 10 s".to_owned());
    })
}

/// Checks the session marks its blocks: `terminal.command_blocks` is on by
/// default and the session is no lane's. Fails the scene if it doesn't.
fn drawn() -> Step {
    Box::new(|workbench, _, cx| {
        let panel = workbench.terminal().read(cx);
        let marked = (panel.view()).is_some_and(|view| view.read(cx).command_blocks().is_some());
        if !marked {
            fail("the terminal session doesn't mark its command blocks".to_owned());
        }
    })
}
