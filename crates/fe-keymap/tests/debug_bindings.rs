//! The debugger's F-keys (docs/decisions/r1b9dw3g0g-debugger.md §6, card
//! D8), as VS Code lays them out, through the workbench root's debug flags:
//! `debug_session` while a session exists, then `debug_stopped` while it's
//! stopped or `debug_running` otherwise. On Linux F10 and F11 step only
//! while stopped, keeping the menu bar and fullscreen otherwise (§13 Q4).

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer, Keystroke,
    Matcher,
};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn macos() -> Keymap {
    keymap(DEFAULT_MACOS, Os::MacOs)
}

fn linux() -> Keymap {
    keymap(DEFAULT_LINUX, Os::Linux)
}

/// The session's state, as the workbench root's flags say it.
#[derive(Clone, Copy, Debug)]
enum Session {
    None,
    Running,
    Stopped,
}

/// The workbench, with `session`'s flags, and inside it `inner` frames.
fn stack(session: Session, inner: &[&str]) -> ContextStack {
    let root = ContextFrame::new("workbench");
    let root = match session {
        Session::None => root,
        Session::Running => root.flag("debug_session").flag("debug_running"),
        Session::Stopped => root.flag("debug_session").flag("debug_stopped"),
    };
    let mut stack = ContextStack::new().with(root);
    for name in inner {
        stack.push(ContextFrame::new(*name));
    }
    stack
}

/// What `keys` runs in `stack`, if anything.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    match Matcher::new(keymap.clone()).push(Keystroke::parse(keys).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

/// §6's table: each key, the command it runs, and the session states it
/// runs it in (in the editor, as in the workbench).
const TABLE: &[(&str, &str, &[Session])] = &[
    (
        "f5",
        "debug.start",
        &[Session::None, Session::Running, Session::Stopped],
    ),
    (
        "shift-f5",
        "debug.stop",
        &[Session::Running, Session::Stopped],
    ),
    (
        "secondary-shift-f5",
        "debug.restart",
        &[Session::Running, Session::Stopped],
    ),
    ("f6", "debug.pause", &[Session::Running]),
    (
        "f9",
        "debug.toggle_breakpoint",
        &[Session::None, Session::Running, Session::Stopped],
    ),
    ("f10", "debug.step_over", &[Session::Stopped]),
    ("f11", "debug.step_into", &[Session::Stopped]),
    ("shift-f11", "debug.step_out", &[Session::Stopped]),
    ("secondary-f10", "debug.run_to_cursor", &[Session::Stopped]),
];

/// `secondary` as `os` names it.
fn on(keys: &str, os: Os) -> String {
    let secondary = match os {
        Os::MacOs => "cmd-",
        _ => "ctrl-",
    };
    keys.replace("secondary-", secondary)
}

#[test]
fn the_f_keys_follow_vs_code_through_the_session_flags() {
    for (keymap, os) in [(macos(), Os::MacOs), (linux(), Os::Linux)] {
        for &(keys, command, states) in TABLE {
            let keys = on(keys, os);
            for session in [Session::None, Session::Running, Session::Stopped] {
                let applies = states
                    .iter()
                    .any(|state| std::mem::discriminant(state) == std::mem::discriminant(&session));
                let ran = run(&keymap, &keys, &stack(session, &["editor"]));
                if applies {
                    assert_eq!(ran.as_deref(), Some(command), "{os:?} {keys} {session:?}");
                } else {
                    assert_ne!(ran.as_deref(), Some(command), "{os:?} {keys} {session:?}");
                }
            }
            // F9 is the editor's; the rest the workbench's.
            let outside = run(&keymap, &keys, &stack(states[0], &[]));
            assert_eq!(
                outside.as_deref() == Some(command),
                command != "debug.toggle_breakpoint",
                "{os:?} {keys} outside the editor"
            );
        }
        // Each command has the one key.
        for &(keys, command, _) in TABLE {
            let bound: Vec<String> = (keymap.bindings().iter())
                .filter(|binding| binding.command.as_str() == command)
                .map(|binding| binding.keys.to_string())
                .collect();
            let want = fe_keymap::KeySequence::parse(&on(keys, os))
                .unwrap()
                .to_string();
            assert_eq!(bound, [want], "{os:?} {command}");
        }
    }
}

#[test]
fn f10_opens_the_menu_bar_on_linux_with_no_session_and_steps_over_while_stopped() {
    let linux = linux();
    for inner in [&[][..], &["editor"][..]] {
        for session in [Session::None, Session::Running] {
            assert_eq!(
                run(&linux, "f10", &stack(session, inner)).as_deref(),
                Some("menu_bar.open"),
                "{session:?} {inner:?}"
            );
        }
        assert_eq!(
            run(&linux, "f10", &stack(Session::Stopped, inner)).as_deref(),
            Some("debug.step_over"),
            "{inner:?}"
        );
    }
    // macOS has no menu bar of its own on F10: it steps while stopped, and
    // is free otherwise.
    let macos = macos();
    assert_eq!(run(&macos, "f10", &stack(Session::None, &["editor"])), None);
    assert_eq!(
        run(&macos, "f10", &stack(Session::Stopped, &["editor"])).as_deref(),
        Some("debug.step_over")
    );
}

#[test]
fn f11_toggles_fullscreen_on_linux_with_no_session_and_steps_into_while_stopped() {
    let linux = linux();
    for inner in [&[][..], &["editor"][..]] {
        for session in [Session::None, Session::Running] {
            assert_eq!(
                run(&linux, "f11", &stack(session, inner)).as_deref(),
                Some("window.toggle_fullscreen"),
                "{session:?} {inner:?}"
            );
        }
        assert_eq!(
            run(&linux, "f11", &stack(Session::Stopped, inner)).as_deref(),
            Some("debug.step_into"),
            "{inner:?}"
        );
    }
    // macOS's fullscreen is ctrl-cmd-f, whatever the session.
    let macos = macos();
    assert_eq!(
        run(&macos, "ctrl-cmd-f", &stack(Session::Stopped, &["editor"])).as_deref(),
        Some("window.toggle_fullscreen")
    );
    assert_eq!(
        run(&macos, "f11", &stack(Session::Running, &["editor"])),
        None
    );
}
