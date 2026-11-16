//! The floating debug toolbar (`docs/design/design-system.md`, Debugger,
//! "Without a title bar"; debugger card D8) over `src/main.rs`, with no
//! title bar, as on Linux and on macOS without the unified one: a session
//! of fe-dap's fake adapter stopped on a breakpoint in the loop's body, so
//! the toolbar shows the configuration's label, Continue, the three steps,
//! Restart and Stop, all enabled, and the gutter the stopped frame.

use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_dap::{
    AdapterSpec, AdapterTransport, SessionState,
    fake::{FakeLauncher, Line, Script},
};
use fe_workbench::debug_session::{DebugHost, set_debug_host};
use gpui::VisualTestAppContext;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "debug_toolbar",
    setup,
    steps: |root| {
        vec![
            prepare(),
            idle(),
            start(root),
            idle(),
            wait(),
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

// The toolbar is centred over the editor area, 4 px below its breadcrumbs:
// x 525 to 757, y 62 to 92, its hairline the outermost pixel.
const PROBES: &[Probe] = &[
    // Its floating surface, in its padding left of the label.
    probe(527.5, 77., "surface.raised"),
    // In its padding below Step Into.
    probe(640., 90., "surface.raised"),
    // Its hairline, along its top edge and its left.
    probe(657., 62.5, "border.default"),
    probe(525.5, 77., "border.default"),
];

/// Installs the fake as the host's adapter `fake`, its program
/// `src/main.rs` line by line, and the launch file naming it.
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

/// Hides the title bar and trusts the folder, which applies once its
/// canonical path is resolved, off the main thread.
fn prepare() -> Step {
    Box::new(|workbench, _, cx| {
        workbench.set_title_bar(false, cx);
        workbench.set_root_trust(true, cx);
    })
}

/// Sets the breakpoint and starts the configuration, in the trusted folder.
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
/// loop's body.
fn check() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(debugger) = workbench.debugger(cx) else {
            return fail("no debugger".to_owned());
        };
        let stopped = (debugger.read(cx).stop_location()).map(|stop| stop.row);
        if stopped != Some(STOPPED) {
            fail(format!("stopped at {stopped:?}, not row {STOPPED}"));
        }
    })
}
