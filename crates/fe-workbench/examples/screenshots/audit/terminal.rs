//! The terminal panel in B3 (T349): three sessions as tabs in its strip,
//! each a fixed session (`TerminalPanel::open_with_session`) of the audit's
//! commands, marked by OSC 133 as a shell integration marks them: one still
//! running (`cargo run`, a spinner on its tab), one whose last command
//! failed (`cargo test`, exit 1, the error mark) and one whose last command
//! passed (`cargo build`, exit 0, the terminal icon). The running one is
//! active. No user shell, profile or setting is in it.
//!
//! The probes are on the tabs, along the strip under the dock's own, 34 px
//! each: the bottom panel is 250 px over the 24 px status bar, so the dock's
//! strip is 326–360 and the panel's 360–394; it starts at the centre's
//! left, after the 42 px rail and the 248 px sidebar. A tab labelled
//! `Terminal <n>` is about 145 px wide: 12 px of padding, the icon, the
//! label, then the close button's 16 px box (shown on the active tab alone)
//! and 8 px of padding.

use std::time::{Duration, Instant};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "terminal",
    steps: |_| {
        vec![
            open(Ends::Running),
            run("terminal.toggle"),
            open(Ends::Failed),
            open(Ends::Passed),
            wait(),
            // From the last tab, round to the first: the running one.
            run("terminal.next"),
            idle(),
        ]
    },
    probes: PROBES,
    ..DEFAULT
};

/// The strip's middle row.
const STRIP_Y: f32 = 377.;

const PROBES: &[Probe] = &[
    // The active tab, in its left padding.
    probe(296., STRIP_Y, "tab.active"),
    // The other two, where an inactive tab's close button hides.
    probe(563., STRIP_Y, "tab.inactive"),
    probe(708., STRIP_Y, "tab.inactive"),
    // The strip after the tabs, before its buttons.
    probe(850., STRIP_Y, "surface.chrome"),
    // The active session, right of its output.
    probe(960., 540., "surface.background"),
];

/// How a session's last command ends.
#[derive(Clone, Copy)]
enum Ends {
    Running,
    Failed,
    Passed,
}

impl Ends {
    /// The last text its session writes: once it shows, all of it has.
    fn last(self) -> &'static str {
        match self {
            Ends::Running => "Listening on http://127.0.0.1:8080",
            Ends::Failed => "test failed, to rerun pass",
            Ends::Passed => "target(s) in 0.42s",
        }
    }

    /// What its session writes: a prompt, the command typed, its output
    /// and its exit code for each command, by OSC 133's A, B, C and D
    /// marks. `cargo build` passes; then `cargo test` fails; then `cargo
    /// run` runs on.
    fn session(self) -> String {
        let prompt = "\x1b]133;A\x07\x1b[1;34m~/demo\x1b[0m \x1b[32m$\x1b[0m \x1b]133;B\x07";
        let output = "\x1b]133;C\x07";
        let done = |code: i32| format!("\x1b]133;D;{code}\x07");
        let build = [
            prompt.to_owned(),
            "cargo build\r\n".to_owned(),
            output.to_owned(),
            "   \x1b[1;32mCompiling\x1b[0m demo v0.1.0 (~/demo)\r\n".to_owned(),
            "    \x1b[1;32mFinished\x1b[0m `dev` profile [unoptimized + debuginfo] \
             target(s) in 0.42s\r\n"
                .to_owned(),
            done(0),
        ];
        let test = [
            prompt.to_owned(),
            "cargo test\r\n".to_owned(),
            output.to_owned(),
            "test counts_words ... \x1b[32mok\x1b[0m\r\n".to_owned(),
            "test counts_lines ... \x1b[31mFAILED\x1b[0m\r\n".to_owned(),
            "\x1b[1;31merror\x1b[0m: test failed, to rerun pass `--bin demo`\r\n".to_owned(),
            done(1),
        ];
        let serve = [
            prompt.to_owned(),
            "cargo run -- serve\r\n".to_owned(),
            output.to_owned(),
            format!("{}\r\n", Ends::Running.last()),
        ];
        let mut session = build.concat();
        if matches!(self, Ends::Failed | Ends::Running) {
            session += &test.concat();
        }
        if matches!(self, Ends::Running) {
            session += &serve.concat();
        }
        session
    }
}

/// Opens a session ending `ends` in the terminal panel, after the others
/// and active: its process starts when the panel next lays it out, on the
/// next step's frame.
fn open(ends: Ends) -> Step {
    Box::new(move |workbench, window, cx| {
        let bytes = ends.session();
        (workbench.terminal().clone())
            .update(cx, |panel, cx| panel.open_with_session(&bytes, window, cx));
    })
}

/// Waits, up to 10 s, for each session's process to have written it all:
/// they run beside the app, so no frame or task waits for them. Fails the
/// scene if one hasn't by then, or a session is missing.
fn wait() -> Step {
    Box::new(|workbench, _, cx| {
        let ends = [Ends::Running, Ends::Failed, Ends::Passed];
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let panel = workbench.terminal().read(cx);
            let views: Vec<_> = panel.views().collect();
            let written = views.len() == ends.len()
                && (views.iter().zip(ends)).all(|(view, end)| {
                    let terminal = view.read(cx).terminal();
                    terminal.is_some_and(|terminal| terminal.output_text().contains(end.last()))
                });
            if written {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fail("the terminal sessions didn't write their output in 10 s".to_owned());
    })
}
