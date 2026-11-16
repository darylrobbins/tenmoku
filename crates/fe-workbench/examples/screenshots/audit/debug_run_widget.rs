//! The title bar's run widget during a debug session
//! (`docs/design/design-system.md`, Debugger, "The run widget"; debugger
//! card D8b): a session of fe-dap's fake adapter stopped on a breakpoint
//! in `src/main.rs`'s loop, so the widget is the session's controls: the
//! configuration's label with the stopped state's `pause` icon, Continue,
//! the three steps, 6 px, Restart and Stop, all enabled, the pointer
//! resting on Restart. The floating toolbar doesn't show: there's a
//! title bar.

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
use gpui::{
    Modifiers, MouseMoveEvent, Pixels, PlatformInput, Point, VisualTestAppContext, point, px,
};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "debug_run_widget",
    setup,
    steps: |root| {
        vec![
            prepare(),
            idle(),
            start(root),
            // Each frame between them runs the start's foreground work.
            wait(),
            wait(),
            wait(),
            check(),
            pointer(RESTART),
        ]
    },
    probes: PROBES,
    // Only the stopped file, as the stop opens it.
    open_demo: false,
    ..DEFAULT
};

/// `src/main.rs`'s row (from 0) the session stops on: the loop's body.
const STOPPED: u32 = 8;

/// The launch file: the fake debugs `demo`.
const LAUNCH: &str = r#"
[[launch]]
name = "demo"
adapter = "fake"
program = "${workspaceFolder}/target/debug/demo"
"#;

/// Within Restart, in the title bar's right group: the widget's buttons
/// run from x 798 (Continue) to 944 (Stop), Restart at x 898 to 920.
const RESTART: Point<Pixels> = point(px(909.), px(19.));

const PROBES: &[Probe] = &[
    // The title bar beside the widget.
    probe(880., 6., "surface.chrome"),
    // The 6 px between Step Out and Restart.
    probe(895., 19., "surface.chrome"),
    // Restart, hovered, above and below its icon.
    probe(900., 10., "list.row_hover"),
    probe(918., 28., "list.row_hover"),
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

/// Trusts the folder, which applies once its canonical path is resolved,
/// off the main thread. The capture shows the title bar.
fn prepare() -> Step {
    Box::new(|workbench, _, cx| {
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
/// runs beside the app, so no frame or task waits for it. While the start
/// is still reading its configuration on the foreground there's no session
/// yet, and blocking here would stop that work: it returns at once, the
/// next frame's step waits again, and [`check`] fails the scene if the
/// session never stopped.
fn wait() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(debugger) = workbench.debugger(cx) else {
            return fail("no debugger".to_owned());
        };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            let debugger = debugger.read(cx);
            let Some(session) = debugger.session() else {
                // Still starting, or never started: the next frame tells.
                return;
            };
            let framed = (session.stop_data()).is_some_and(|stop| !stop.frames.is_empty());
            if session.state() == SessionState::Stopped && framed {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })
}

/// Fails the scene unless the last frame had the session stopped on the
/// loop's body.
fn check() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(debugger) = workbench.debugger(cx) else {
            return fail("no debugger".to_owned());
        };
        let debugger = debugger.read(cx);
        if debugger.session().is_none() {
            let starting = debugger.starting().is_some();
            return fail(format!("no session (still starting: {starting})"));
        }
        let stopped = (debugger.stop_location()).map(|stop| stop.row);
        if stopped != Some(STOPPED) {
            fail(format!("stopped at {stopped:?}, not row {STOPPED}"));
        }
    })
}

/// Moves the pointer to `at`, once the step's update of the workbench is
/// over, so what's under it shows hovered.
fn pointer(at: Point<Pixels>) -> Step {
    Box::new(move |_, window, cx| {
        window.defer(cx, move |window, cx| {
            let event = PlatformInput::MouseMove(MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers: Modifiers::none(),
            });
            window.dispatch_event(event, cx);
        });
    })
}
