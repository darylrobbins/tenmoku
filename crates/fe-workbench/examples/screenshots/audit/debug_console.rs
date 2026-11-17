//! The Debug Console (`docs/design/design-system.md`, Debugger, "Console";
//! debugger card D10) on the Debug tool window's Console tab: a session of
//! fe-dap's fake adapter that wrote mixed output as it started (the
//! adapter's own message, program output, two lines of its error stream
//! and an important message), stopped on the loop's body of `src/main.rs`,
//! then an evaluation whose result is expanded and one that fails.

use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_command::Invocation;
use fe_dap::{
    AdapterSpec, AdapterTransport, SessionState,
    fake::{FakeLauncher, Function, Line, Local, Script},
    protocol::OutputBody,
};
use fe_gpui::commands::RunCommand;
use fe_workbench::debug_session::{DebugHost, set_debug_host};
use gpui::{VisualTestAppContext, px};
use kiln_shell::DockView as _;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "debug_console",
    setup,
    steps: |root| {
        vec![
            prepare(),
            idle(),
            start(root),
            idle(),
            wait(),
            evaluate(),
            answers(),
            check(),
        ]
    },
    probes: PROBES,
    // Only the stopped file, as the stop opens it.
    open_demo: false,
    ..DEFAULT
};

/// `src/main.rs`'s row (from 0) the session stops on: the loop's body.
const STOPPED: u32 = 8;

/// The bottom panel's height here: room for every row.
const PANEL_HEIGHT: f32 = 340.;

/// The launch file: the fake debugs `main`.
const LAUNCH: &str = r#"
[[launch]]
name = "demo"
adapter = "fake"
program = "${workspaceFolder}/target/debug/demo"
"#;

// The bottom panel starts right of the 240 px sidebar, at x 282, and 340
// px above the 24 px status bar, at y 236: the dock's 34 px tab strip, the
// tool window's 26 px row of tabs, then the console's list, 4 px in, of
// 18 px rows (y 301 on), the input's 26 px at the foot under a hairline at
// y 550. The error stream's two rows are the list's fourth and fifth, y
// 356 to 392, their 2 px edge at x 282 to 284.
const PROBES: &[Probe] = &[
    // The tool window's ground, right of the rows' text.
    probe(900., 340., "surface.panel"),
    // The error stream's edge, on its second row.
    probe(283., 383., "status.error"),
    // The hairline over the input.
    probe(900., 550., "border.subtle"),
];

fn out(category: &str, text: &str) -> OutputBody {
    OutputBody {
        output: text.to_owned(),
        category: Some(category.to_owned()),
        ..Default::default()
    }
}

/// Installs the fake as the host's adapter `fake`, its program
/// `src/main.rs` line by line with `main`'s locals and its output, and the
/// launch file naming it.
fn setup(cx: &mut VisualTestAppContext, parent: &Path) {
    let root = parent.join("demo");
    // As the breakpoint store and the session send it: resolved.
    let canonical = dunce::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let mut script = Script::default();
    script.program.path = (canonical.join("src/main.rs").to_string_lossy()).into_owned();
    script.program.lines = (MAIN_RS.lines())
        .map(|text| Line {
            text: text.to_owned(),
            function: "main".to_owned(),
            spin: false,
        })
        .collect();
    let counts = Local {
        children: vec![
            Local::new("\"alpha beta\"", "2", "usize"),
            Local::new("\"gamma\"", "1", "usize"),
        ],
        ..Local::new(
            "counts",
            "{\"alpha beta\": 2, \"gamma\": 1}",
            "HashMap<String, usize>",
        )
    };
    script.program.functions = vec![Function {
        name: "main".to_owned(),
        locals: vec![counts, Local::new("line", "\"gamma\"", "String")],
    }];
    script.output = vec![
        out("console", "Launching demo with the fake adapter\n"),
        out("stdout", "Reading words.txt\n"),
        out("stdout", "alpha beta: 2\n"),
        out("stderr", "warning: line 3 is empty\n  skipped it\n"),
        out("important", "Breakpoint moved to line 9\n"),
        out("stdout", "gamma: 1\n"),
    ];
    let host = DebugHost {
        launcher: Some(Arc::new(FakeLauncher::new(script))),
        specs: vec![AdapterSpec {
            types: vec!["fake".into()],
            languages: vec!["rust".into()],
            transport: AdapterTransport::Port(1),
            ..AdapterSpec::new("fake", "fake")
        }],
        ..DebugHost::default()
    };
    cx.update(|cx| set_debug_host(cx, host));
    let written = fs::create_dir_all(root.join(".tenmoku"))
        .and_then(|()| fs::write(root.join(".tenmoku/launch.toml"), LAUNCH));
    if let Err(error) = written {
        fail(format!("the launch file: {error}"));
    }
}

/// Trusts the folder, which applies once its canonical path is resolved,
/// off the main thread.
fn prepare() -> Step {
    Box::new(|workbench, _, cx| workbench.set_root_trust(true, cx))
}

/// Sets the breakpoint and the bottom panel's height, and starts the
/// configuration, in the trusted folder.
fn start(root: &Path) -> Step {
    let main = root.join("src/main.rs");
    Box::new(move |workbench, window, cx| {
        if !workbench.root_trusted() {
            return fail("the folder isn't trusted yet".to_owned());
        }
        let Some(store) = workbench.breakpoint_store(cx) else {
            return fail("no breakpoint store".to_owned());
        };
        store.update(cx, |store, cx| store.toggle(&main, STOPPED, cx));
        (workbench.terminal_dock().clone()).update(cx, |dock, cx| {
            dock.dock_mut().set_size(px(PANEL_HEIGHT));
            cx.notify();
        });
        workbench.start_debugging("demo", window, cx);
    })
}

/// How long a step gives the fake, which runs beside the app, to answer
/// what the step asked: its answers land before the next frame.
const ANSWER: Duration = Duration::from_millis(300);

/// Waits, up to 10 s, for the session to stop with its frames: the fake
/// runs beside the app, so no frame or task waits for it. Fails the scene
/// if it hasn't started or stopped by then. Then shows the Console, as
/// `debug.show_console` does.
fn wait() -> Step {
    Box::new(|workbench, window, cx| {
        let Some(debugger) = workbench.debugger(cx) else {
            return fail("no debugger".to_owned());
        };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let debugger = debugger.read(cx);
            let Some(session) = debugger.session() else {
                return fail("the session didn't start".to_owned());
            };
            let framed = (session.stop_data()).is_some_and(|stop| !stop.frames.is_empty());
            if session.state() == SessionState::Stopped && framed {
                let context = kiln_platform::keys::context_stack(&window.context_stack(), cx);
                let invocation = Invocation::of("debug.show_console").expect("a command ID");
                let run = RunCommand(invocation.context(context));
                return window.dispatch_action(Box::new(run), cx);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fail("the session didn't stop within 10 s".to_owned());
    })
}

/// Evaluates `counts`, its result to expand, and `total`, which the frame
/// doesn't have.
fn evaluate() -> Step {
    Box::new(|workbench, _, cx| {
        workbench.evaluate_in_debug_console("counts", true, cx);
        workbench.evaluate_in_debug_console("total", false, cx);
        std::thread::sleep(ANSWER);
    })
}

/// A frame for the answers to land, which asks for the expanded result's
/// children, and time for those to be answered.
fn answers() -> Step {
    Box::new(|_, _, _| std::thread::sleep(ANSWER))
}

/// Fails the scene unless the evaluations were answered and the result's
/// children came, the session stopped on the loop's body and the bottom
/// panel shows the Debug tool window.
fn check() -> Step {
    Box::new(|workbench, _, cx| {
        if !workbench.debug_console_settled(cx) {
            return fail("the console's evaluations weren't all answered".to_owned());
        }
        let Some(debugger) = workbench.debugger(cx) else {
            return fail("no debugger".to_owned());
        };
        let stopped = (debugger.read(cx).stop_location()).map(|stop| stop.row);
        if stopped != Some(STOPPED) {
            return fail(format!("stopped at {stopped:?}, not row {STOPPED}"));
        }
        let dock = workbench.terminal_dock().read(cx);
        let shown = (dock.is_visible())
            .then(|| dock.dock().rail_highlighted())
            .flatten();
        if shown.map(|panel| panel.0) != Some("debug") {
            fail(format!("the bottom panel shows {shown:?}, not Debug"));
        }
    })
}
