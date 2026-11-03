//! The subword moves' and deletes' keys: ctrl-alt chords on macOS, unbound
//! elsewhere, where ctrl-alt-left and right move tabs.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, KeySequence, Keymap, KeymapLayer};
use kiln_core::Os;

const MOVES: [(&str, &str); 4] = [
    ("ctrl-alt-left", "editor.move_subword_left"),
    ("ctrl-alt-right", "editor.move_subword_right"),
    ("ctrl-alt-shift-left", "editor.select_subword_left"),
    ("ctrl-alt-shift-right", "editor.select_subword_right"),
];

const DELETES: [(&str, &str); 2] = [
    ("ctrl-alt-backspace", "editor.delete_subword_left"),
    ("ctrl-alt-delete", "editor.delete_subword_right"),
];

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frame: ContextFrame) -> ContextStack {
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

fn editor() -> ContextStack {
    stack(ContextFrame::new("editor"))
}

fn vim(mode: &str) -> ContextStack {
    stack(
        ContextFrame::new("editor")
            .flag("vim.enabled")
            .set("vim.mode", mode),
    )
}

/// What `keys` runs in `stack`: the last binding of exactly that key whose
/// context holds.
fn resolve(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
    let keys = KeySequence::parse_for(keys, os).unwrap();
    keymap
        .bindings()
        .iter()
        .rev()
        .find(|binding| {
            binding.keys == keys && binding.context.as_ref().is_none_or(|c| c.eval(stack))
        })
        .map(|binding| binding.command.as_str().to_owned())
}

#[test]
fn macos_binds_the_six_in_the_editor() {
    let mac = keymap(Os::MacOs);
    for (keys, command) in MOVES.into_iter().chain(DELETES) {
        assert_eq!(
            resolve(&mac, Os::MacOs, keys, &editor()).as_deref(),
            Some(command),
            "{keys}"
        );
    }
}

#[test]
fn macos_leaves_them_alone_outside_the_editor() {
    let mac = keymap(Os::MacOs);
    let elsewhere = stack(ContextFrame::new("project_panel"));
    for (keys, _) in MOVES.into_iter().chain(DELETES) {
        assert_eq!(resolve(&mac, Os::MacOs, keys, &elsewhere), None, "{keys}");
    }
}

#[test]
fn the_deletes_stay_out_of_vim_normal_mode() {
    let mac = keymap(Os::MacOs);
    for (keys, command) in DELETES {
        assert_eq!(
            resolve(&mac, Os::MacOs, keys, &vim("normal")),
            None,
            "{keys}"
        );
        for mode in ["insert", "replace"] {
            assert_eq!(
                resolve(&mac, Os::MacOs, keys, &vim(mode)).as_deref(),
                Some(command),
                "{keys} in vim {mode}"
            );
        }
    }
}

#[test]
fn macos_keeps_moving_tabs_on_its_own_keys() {
    let mac = keymap(Os::MacOs);
    let workbench = stack(ContextFrame::new("project_panel"));
    for (keys, command) in [
        ("ctrl-cmd-left", "pane.move_tab_left"),
        ("ctrl-cmd-right", "pane.move_tab_right"),
    ] {
        assert_eq!(
            resolve(&mac, Os::MacOs, keys, &workbench).as_deref(),
            Some(command),
            "{keys}"
        );
    }
}

#[test]
fn off_macos_none_is_bound_and_ctrl_alt_left_moves_the_tab() {
    for os in [Os::Linux, Os::Windows] {
        let keymap = keymap(os);
        for (_, command) in MOVES.into_iter().chain(DELETES) {
            assert!(
                keymap
                    .bindings()
                    .iter()
                    .all(|binding| binding.command.as_str() != command),
                "{command} on {os:?}"
            );
        }
        for (keys, command) in [
            ("ctrl-alt-left", "pane.move_tab_left"),
            ("ctrl-alt-right", "pane.move_tab_right"),
        ] {
            assert_eq!(
                resolve(&keymap, os, keys, &editor()).as_deref(),
                Some(command),
                "{keys} on {os:?}"
            );
        }
        for keys in [
            "ctrl-alt-shift-left",
            "ctrl-alt-shift-right",
            "ctrl-alt-backspace",
            "ctrl-alt-delete",
        ] {
            assert_eq!(
                resolve(&keymap, os, keys, &editor()),
                None,
                "{keys} on {os:?}"
            );
        }
    }
}
