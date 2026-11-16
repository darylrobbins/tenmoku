//! The floating debug toolbar (`debug_toolbar.rs`, card D8) against
//! fe-dap's fake adapter: its buttons' enabled states in each session
//! state, as the control-state query has them; where it shows, with and
//! without a title bar; and its buttons' accessible names.

use fe_dap::{
    Control, SessionState,
    fake::{Fault, FaultKind, Script},
};
use gpui::{TestAppContext, px};
use kiln_l10n::tr;

use super::debug_commands::{Debug, main_path, script, spinning, trusted};
use crate::{
    debug_session::{DEBUG_RUNNING, DEBUG_SESSION, DEBUG_STOPPED},
    debug_toolbar::{DRAWN, Drawn, TOOLBAR_ID},
};

/// The debugger's key context flags now.
fn flags(debug: &mut Debug) -> Vec<&'static str> {
    let debugger = debug.debugger();
    debugger.read_with(debug.cx, |debugger, _| debugger.key_flags())
}

/// What `keys` runs in Linux's default keymap, in the window's focused
/// context as the last frame left it: the workbench root's debug flags
/// with it.
fn on_linux(debug: &mut Debug, keys: &str) -> Option<String> {
    debug.draw();
    let stack = debug
        .cx
        .update(|window, cx| kiln::platform::keys::context_stack(&window.context_stack(), cx));
    let (layer, diagnostics) = fe_keymap::KeymapLayer::parse_for(
        std::path::Path::new(fe_keymap::DEFAULT_KEYMAP_PATH),
        fe_keymap::DEFAULT_LINUX,
        None,
        kiln::core::Os::Linux,
    );
    assert_eq!(diagnostics, []);
    let keymap = fe_keymap::Keymap::from_layers(vec![layer.unwrap()]);
    let keystroke = fe_keymap::Keystroke::parse(keys).unwrap();
    match fe_keymap::Matcher::new(keymap).push(keystroke, &stack) {
        fe_keymap::KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

/// Review F3 (§13 Q4): F10 and F11 step on Linux only while the session is
/// stopped; resuming, uncertain or running they're the menu bar's and
/// fullscreen's.
fn assert_f_keys(debug: &mut Debug, stopped: bool, state: &str) {
    let (f10, f11) = match stopped {
        true => ("debug.step_over", "debug.step_into"),
        false => ("menu_bar.open", "window.toggle_fullscreen"),
    };
    assert_eq!(on_linux(debug, "f10").as_deref(), Some(f10), "{state}");
    assert_eq!(on_linux(debug, "f11").as_deref(), Some(f11), "{state}");
    let flag = if stopped {
        DEBUG_STOPPED
    } else {
        DEBUG_RUNNING
    };
    assert_eq!(flags(debug), [DEBUG_SESSION, flag], "{state}");
}

/// What the last frame's toolbar drew.
fn drawn(debug: &mut Debug) -> Option<Drawn> {
    debug.draw();
    DRAWN.with_borrow(Clone::clone)
}

/// Each button the toolbar drew: its id and whether it's enabled.
fn enabled(debug: &mut Debug) -> Vec<(&'static str, bool)> {
    let drawn = drawn(debug).expect("the toolbar shows");
    (drawn.buttons.iter())
        .map(|(id, _, enabled, _)| (*id, *enabled))
        .collect()
}

/// The buttons the control-state query says are enabled now, in the
/// toolbar's order, for those it drew.
fn from_controls(debug: &mut Debug, ids: &[(&'static str, bool)]) -> Vec<(&'static str, bool)> {
    let debugger = debug.debugger();
    let controls = debugger.read_with(debug.cx, |debugger, _| debugger.controls());
    let control = |id: &str| -> Control {
        match id {
            "debug-toolbar-continue" => controls.continue_,
            "debug-toolbar-pause" => controls.pause,
            "debug-toolbar-restart" => controls.restart,
            "debug-toolbar-stop" => controls.stop,
            _ => controls.step,
        }
    };
    (ids.iter())
        .map(|(id, _)| (*id, control(id).is_enabled()))
        .collect()
}

const CONTINUE: &str = "debug-toolbar-continue";
const PAUSE: &str = "debug-toolbar-pause";
const STEP_OVER: &str = "debug-toolbar-step-over";
const STEP_INTO: &str = "debug-toolbar-step-into";
const STEP_OUT: &str = "debug-toolbar-step-out";
const RESTART: &str = "debug-toolbar-restart";
const STOP: &str = "debug-toolbar-stop";

/// C3: Stopped, Resuming, Uncertain and Running, each as the control-state
/// query says: a step that's never answered resumes until it times out,
/// which leaves the session uncertain, and Continue from there runs the
/// program, which spins.
#[gpui::test]
fn disabled_states_follow_the_session(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| Script {
        faults: vec![Fault::new("next", FaultKind::Unanswered)],
        ..spinning(script(root))
    });
    debug.set_request_timeout(2_000);
    let main = main_path(&debug.root);
    debug.toggle(&main, 0);
    assert!(drawn(&mut debug).is_none(), "no session, no toolbar");
    assert_eq!(flags(&mut debug), Vec::<&str>::new());
    assert_eq!(
        on_linux(&mut debug, "f10").as_deref(),
        Some("menu_bar.open")
    );
    debug.start("main");
    debug.wait_stop_at(0);

    let stopped = enabled(&mut debug);
    assert_eq!(
        stopped,
        [
            (CONTINUE, true),
            (STEP_OVER, true),
            (STEP_INTO, true),
            (STEP_OUT, true),
            (RESTART, true),
            (STOP, true),
        ],
        "stopped"
    );
    assert_eq!(from_controls(&mut debug, &stopped), stopped);
    assert_f_keys(&mut debug, true, "stopped");

    debug.run("debug.step_over");
    debug.wait_state("resuming", |state| *state == SessionState::Resuming);
    let resuming = enabled(&mut debug);
    assert_eq!(
        resuming,
        [
            (PAUSE, true),
            (STEP_OVER, false),
            (STEP_INTO, false),
            (STEP_OUT, false),
            (RESTART, false),
            (STOP, true),
        ],
        "resuming"
    );
    assert_eq!(from_controls(&mut debug, &resuming), resuming);
    assert_f_keys(&mut debug, false, "resuming");

    debug.wait_state("uncertain", |state| *state == SessionState::Uncertain);
    let uncertain = enabled(&mut debug);
    assert_eq!(
        uncertain,
        [
            (CONTINUE, true),
            (PAUSE, true),
            (STEP_OVER, false),
            (STEP_INTO, false),
            (STEP_OUT, false),
            (RESTART, false),
            (STOP, true),
        ],
        "uncertain"
    );
    assert_eq!(from_controls(&mut debug, &uncertain), uncertain);
    assert_f_keys(&mut debug, false, "uncertain");

    debug.run("debug.continue");
    debug.wait_state("running", |state| *state == SessionState::Running);
    let running = enabled(&mut debug);
    assert_eq!(
        running,
        [
            (PAUSE, true),
            (STEP_OVER, false),
            (STEP_INTO, false),
            (STEP_OUT, false),
            (RESTART, true),
            (STOP, true),
        ],
        "running"
    );
    assert_eq!(from_controls(&mut debug, &running), running);
    assert_f_keys(&mut debug, false, "running");

    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
    assert!(drawn(&mut debug).is_none(), "an ended session, no toolbar");
    assert_eq!(flags(&mut debug), Vec::<&str>::new());
}

/// Review F2: a picker opens over the panes' top, where the toolbar floats,
/// so the toolbar gives way while one shows, and comes back after.
#[gpui::test]
fn the_toolbar_gives_way_to_a_picker(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    assert!(drawn(&mut debug).is_some());
    for picker in ["debug.show_session", "debug.start_with_args"] {
        debug.run(picker);
        assert!(drawn(&mut debug).is_none(), "{picker}");
        assert_eq!(debug.cx.debug_bounds(TOOLBAR_ID), None, "{picker}");
        debug.cx.simulate_keystrokes("escape");
        assert!(drawn(&mut debug).is_some(), "after {picker}");
    }
}

/// C4: the toolbar shows while a session exists and there's no title bar
/// (Linux, which has none, and macOS without the unified one: the same
/// test, whether there's a title bar), centred over the editor area 4 px
/// below its tab strip. With a title bar it doesn't: the controls are the
/// title bar's run widget's there (D0).
#[gpui::test]
fn the_toolbar_shows_without_a_title_bar(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    assert!(drawn(&mut debug).is_some());
    let bar = debug.cx.debug_bounds(TOOLBAR_ID).expect("drawn");
    let workbench = debug.workbench.clone();
    let area = debug
        .cx
        .update(|_, cx| workbench.read(cx).editor_area(cx))
        .expect("an editor area");
    assert_eq!(bar.top(), area.top() + px(4.));
    assert!(
        (bar.center().x - area.center().x).abs() <= px(0.5),
        "{bar:?} in {area:?}"
    );
    assert_eq!(bar.size.height, px(30.));

    // With the title bar, the controls are the run widget's.
    workbench.update(debug.cx, |workbench, cx| workbench.set_title_bar(true, cx));
    assert!(drawn(&mut debug).is_none());
    assert_eq!(debug.cx.debug_bounds(TOOLBAR_ID), None);
    workbench.update(debug.cx, |workbench, cx| workbench.set_title_bar(false, cx));
    assert!(drawn(&mut debug).is_some());

    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
    assert!(drawn(&mut debug).is_none());
    assert_eq!(debug.cx.debug_bounds(TOOLBAR_ID), None);
}

/// C5: every button names itself, the toolbar's tooltip names with their
/// keys, and the label names the session and where it's stopped.
#[gpui::test]
fn buttons_have_accessible_names(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| spinning(script(root)));
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.start("main");
    debug.wait_stop_at(2);
    let drawn_now = drawn(&mut debug).expect("the toolbar shows");
    let names: Vec<(String, Option<String>)> = (drawn_now.buttons.iter())
        .map(|(_, name, _, key)| (name.to_string(), key.clone()))
        .collect();
    let named =
        |key: &str, shortcut: Option<&str>| (tr!(key).to_string(), shortcut.map(ToOwned::to_owned));
    // fe-keymap's labels, as the keymap binds them on this platform.
    let label = |keys: &str| {
        let keys = fe_keymap::KeySequence::parse(keys).unwrap();
        fe_keymap::shortcut_label(&keys)
    };
    // The test keymap is fe_gpui's DEFAULT_MACOS on every platform, so
    // `secondary-` is cmd here, printed as Super off macOS.
    let restart = label("cmd-shift-f5");
    let (f5, stop, f10, f11, step_out) = (
        label("f5"),
        label("shift-f5"),
        label("f10"),
        label("f11"),
        label("shift-f11"),
    );
    assert_eq!(
        names,
        [
            // Continue's is F5's, which continues while stopped.
            named("workbench-debug-toolbar-continue", Some(&f5)),
            named("workbench-debug-toolbar-step-over", Some(&f10)),
            named("workbench-debug-toolbar-step-into", Some(&f11)),
            named("workbench-debug-toolbar-step-out", Some(&step_out)),
            named("workbench-debug-toolbar-restart", Some(&restart)),
            named("workbench-debug-toolbar-stop", Some(&stop)),
        ]
    );
    assert_eq!(
        names
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "Continue",
            "Step Over",
            "Step Into",
            "Step Out",
            "Restart",
            "Stop Debugging"
        ]
    );
    let session = tr!(
        "workbench-debug-toolbar-session-stopped-at",
        name = "main",
        line = 3
    )
    .to_string();
    assert_eq!(drawn_now.label, session);
    assert_eq!(session, "Debug session: main, stopped at line 3");

    // A screen reader hears each of them.
    let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
    let buttons: Vec<String> = (nodes.iter())
        .filter(|node| node.role == fe_ui::Role::Button)
        .map(|node| node.label.to_string())
        .collect();
    for (name, _) in &names {
        assert!(buttons.contains(name), "{name} in {buttons:?}");
    }
    assert!(buttons.contains(&session), "{session} in {buttons:?}");

    // Running, Pause names itself with its key, F6.
    debug.run("debug.continue");
    debug.wait_state("running", |state| *state == SessionState::Running);
    let running = drawn(&mut debug).expect("the toolbar shows");
    assert_eq!(
        running.buttons[0].1.to_string(),
        tr!("workbench-debug-toolbar-pause").to_string()
    );
    assert_eq!(running.buttons[0].3, Some(label("f6")));
    assert_eq!(
        running.label.to_string(),
        tr!("workbench-debug-toolbar-session-running", name = "main").to_string()
    );
}
