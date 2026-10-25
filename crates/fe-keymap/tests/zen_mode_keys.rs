//! Zen Mode's keys: `secondary-k z` toggles it in the workbench, inside the
//! panes' `zen_mode` frame too, where escape starts no chord (VS Code's
//! `escape escape` is T1173's); and its View menu item after Toggle Full
//! Screen.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, KeySequence, Keymap, KeymapLayer, Matcher,
};
use kiln_core::Os;

const COMMAND: &str = "workbench.toggle_zen_mode";

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench, then in zen the panes' `zen_mode` frame, then the editor.
fn stack(zen: bool) -> ContextStack {
    let mut stack = ContextStack::new().with(ContextFrame::new("workbench"));
    if zen {
        stack.push(ContextFrame::new("zen_mode"));
    }
    stack.with(ContextFrame::new("editor"))
}

/// What pressing `keys` runs in `stack`, through the matcher as typed; a
/// chord still pending at the end times out.
fn run(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Unmatched(Vec::new());
    for keystroke in KeySequence::parse_for(keys, os).unwrap().keystrokes() {
        outcome = matcher.push(*keystroke, stack);
    }
    if outcome == KeyOutcome::Pending {
        outcome = matcher.flush();
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn secondary_k_z_toggles_and_no_escape_chord_is_bound() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for zen in [false, true] {
            let toggled = run(&keymap, os, "secondary-k z", &stack(zen));
            assert_eq!(toggled.as_deref(), Some(COMMAND), "{os:?} zen {zen}");
        }
        // VS Code's escape escape would hold every lone escape back.
        let escape = KeySequence::parse_for("escape", os).unwrap();
        let escape = escape.keystrokes()[0];
        let chords: Vec<String> = (keymap.bindings().iter())
            .filter(|binding| {
                let keys = binding.keys.keystrokes();
                keys.len() > 1 && keys[0] == escape
            })
            .map(|binding| format!("{} {}", binding.keys, binding.command.as_str()))
            .collect();
        assert_eq!(chords, Vec::<String>::new(), "{os:?}");
    }
}

/// A lone escape in zen runs at once what it does anywhere else: closing
/// the find bar, or the editor's own.
#[test]
fn a_lone_escape_in_zen_runs_at_once() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let escape = KeySequence::parse_for("escape", os).unwrap();
        let escape = escape.keystrokes()[0];
        let mut find = stack(true);
        find.push(ContextFrame::new("find_bar"));
        for (stack, command) in [
            (find, "find.close"),
            (stack(true), "editor.single_selection"),
        ] {
            let mut matcher = Matcher::new(keymap.clone());
            let KeyOutcome::Dispatch(ran) = matcher.push(escape, &stack) else {
                panic!("{os:?}: a lone escape runs at once");
            };
            assert_eq!(ran.command.as_str(), command, "{os:?}");
        }
    }
}

#[test]
fn the_view_menu_lists_it_after_full_screen() {
    let (macos, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    for bar in [macos.unwrap(), linux.unwrap()] {
        let view = bar.menu("View").unwrap();
        let ids: Vec<&str> = (view.items.iter())
            .map(|item| item.id().unwrap_or("-"))
            .collect();
        let full_screen = ids.iter().position(|id| *id == "window.toggle_fullscreen");
        assert_eq!(ids[full_screen.unwrap() + 1], COMMAND, "{ids:?}");
        let item = view.item(COMMAND).and_then(|item| item.command()).unwrap();
        assert_eq!(item.title, "Toggle Zen Mode");
    }
}
