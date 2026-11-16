//! The Debug tool window (`docs/design/design-system.md`, Debugger, "The
//! Debug tool window"; debugger card D9) in the bottom panel, on its
//! Variables tab: a session of fe-dap's fake adapter stopped on a
//! breakpoint in the loop's body of `src/main.rs`, so Call Stack shows the
//! stopped thread and its frame, Watch a watch with a value and one that
//! fails, and Variables the frame's Locals expanded.

use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_dap::{
    AdapterSpec, AdapterTransport, SessionState,
    fake::{FakeLauncher, Function, Line, Local, Script},
};
use fe_workbench::debug_session::{DebugHost, set_debug_host};
use gpui::VisualTestAppContext;
use kiln_shell::DockView as _;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "debug_panel",
    setup,
    steps: |root| {
        vec![
            prepare(),
            idle(),
            start(root),
            idle(),
            wait(),
            idle(),
            idle(),
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

/// The launch file: the fake debugs `main`.
const LAUNCH: &str = r#"
[[launch]]
name = "demo"
adapter = "fake"
program = "${workspaceFolder}/target/debug/demo"
"#;

// The bottom panel starts right of the 240 px sidebar, at x 282, its
// tool window's 26 px tab row under the dock's tab strip; Call Stack is its
// first 280 px, a hairline at its right edge, the stopped thread's row and
// then its frame's, selected, at y 411 to 435.
const PROBES: &[Probe] = &[
    // The tool window's ground, in Call Stack under its rows.
    probe(400., 500., "surface.panel"),
    // Call Stack's hairline.
    probe(561., 500., "border.subtle"),
    // The selected frame's row, between its function and its place.
    probe(540., 423., "list.row_selected_inactive"),
];

/// Installs the fake as the host's adapter `fake`, its program
/// `src/main.rs` line by line with `main`'s locals, and the launch file
/// naming it.
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

/// Sets the breakpoint and the watches, and starts the configuration, in
/// the trusted folder.
fn start(root: &Path) -> Step {
    let main = root.join("src/main.rs");
    Box::new(move |workbench, window, cx| {
        if !workbench.root_trusted() {
            return fail("the folder isn't trusted yet".to_owned());
        }
        let Some(store) = workbench.breakpoint_store(cx) else {
            return fail("no breakpoint store".to_owned());
        };
        store.update(cx, |store, cx| {
            store.toggle(&main, STOPPED, cx);
            store.set_watch_expressions(vec!["line".to_owned(), "total".to_owned()], cx);
        });
        workbench.start_debugging("demo", window, cx);
    })
}

/// Waits, up to 10 s, for the session to stop with its frames: the fake
/// runs beside the app, so no frame or task waits for it. Fails the scene
/// if it hasn't started or stopped by then.
fn wait() -> Step {
    Box::new(|workbench, _, cx| {
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
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fail("the session didn't stop within 10 s".to_owned());
    })
}

/// Fails the scene unless the last frame had the session stopped on the
/// loop's body, the bottom panel showing the Debug tool window.
fn check() -> Step {
    Box::new(|workbench, _, cx| {
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
