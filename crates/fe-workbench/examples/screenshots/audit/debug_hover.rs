//! Hover evaluation while stopped (`docs/design/design-system.md`, Debugger;
//! debugger card D11): a session of fe-dap's fake adapter, able to evaluate
//! for hovers, stopped in `main` of `src/main.rs`, and the popover of the
//! value of `counts` at the caret: its type, its value and its children.
//! The scene never skips on another scale (`SCREENSHOTS_ALLOW_OTHER_SCALE`
//! is no reason to).

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
use fe_gpui::HoverContent;
use gpui::VisualTestAppContext;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, select},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};
use fe_workbench::debug_session::{DebugHost, set_debug_host};

pub(crate) const SURFACE: Surface = Surface {
    name: "debug_hover",
    setup,
    steps: |root| {
        vec![
            prepare(),
            idle(),
            start(root),
            idle(),
            wait(),
            select("counts", 0),
            show(),
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

/// The markdown the debugger's part of a hover is: the type, the value, the
/// children.
const MARKDOWN: &str = "```\nHashMap<String, usize>\n```\n\n```\ncounts = {\"alpha beta\": 2, \"gamma\": 1}\n```\n\n```\n\"alpha beta\" = 2\n\"gamma\" = 1\n```";

/// The popover's text, as the editor's text hook reads it.
const SHOWN: &str = "HashMap<String, usize>\ncounts = {\"alpha beta\": 2, \"gamma\": 1}\n\"alpha beta\" = 2\n\"gamma\" = 1";

// The popover opens below the caret on `counts` (row 7 of the file, at y 200
// in the editor): its ground, right of the type line, is `surface.raised`.
const PROBES: &[Probe] = &[probe(780., 240., "surface.raised")];

/// Installs the fake as the host's adapter `fake`: its program
/// `src/main.rs` line by line, `main`'s locals, and `supportsEvaluateForHovers`.
fn setup(cx: &mut VisualTestAppContext, parent: &Path) {
    let root = parent.join("demo");
    let canonical = dunce::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let mut script = Script::default();
    script.capabilities.supports_evaluate_for_hovers = Some(true);
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

/// Trusts the folder.
fn prepare() -> Step {
    Box::new(|workbench, _, cx| workbench.set_root_trust(true, cx))
}

/// Sets the breakpoint and starts the configuration.
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

/// Waits, up to 10 s, for the session to stop with its frames.
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

/// The popover `debug.hover` shows for the word the caret ends, set as the
/// hover provider's answer is: the fake answers beside the app, and its
/// answer wakes nothing, so a frame-chained scene can't wait for it. The
/// text is what `tests::debug_hover` reads through the same hook.
fn show() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return fail("no active editor to hover in".to_owned());
        };
        let text = MARKDOWN.to_owned();
        editor.update(cx, |editor, cx| {
            editor.show_popover(HoverContent { text, range: None }, cx)
        });
    })
}

/// Fails the scene unless the popover shows the value and its children.
fn check() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return fail("no active editor".to_owned());
        };
        let shown = editor.read(cx).popover_text();
        if shown.as_deref() != Some(SHOWN) {
            let view = editor.read(cx);
            let tree = view.syntax().is_some_and(|syntax| syntax.tree().is_some());
            let selected = view.model().primary_selection().range;
            fail(format!(
                "the popover shows {shown:?}, not {SHOWN:?} (tree {tree}, selected {selected:?})"
            ));
        }
    })
}
