//! The hex viewer's keys, on macOS and Linux: bound in the `HexViewer`
//! context, and leaving the editor's keys alone.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, Keymap, KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[&str]) -> ContextStack {
    frames.iter().map(|name| ContextFrame::new(*name)).collect()
}

/// What typing `key` runs in `stack`.
fn run(keymap: &Keymap, key: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(key).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

/// The hex viewer's keys on `os`, and what each runs there.
fn hex_keys(os: Os) -> Vec<(&'static str, &'static str)> {
    match os {
        Os::MacOs => vec![
            ("cmd-f", "hex.find"),
            ("ctrl-g", "hex.go_to_offset"),
            ("cmd-c", "hex.copy"),
            ("cmd-g", "hex.find_next"),
            ("cmd-shift-g", "hex.find_previous"),
        ],
        _ => vec![
            ("ctrl-f", "hex.find"),
            ("ctrl-g", "hex.go_to_offset"),
            ("ctrl-c", "hex.copy"),
            ("f3", "hex.find_next"),
            ("shift-f3", "hex.find_previous"),
        ],
    }
}

#[test]
fn the_hex_keys_resolve_in_the_hex_viewer_on_each_platform() {
    let viewer = stack(&["workbench", "pane", "HexViewer"]);
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for (keys, command) in hex_keys(os) {
            assert_eq!(
                run(&keymap, keys, &viewer).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
    }
}

#[test]
fn the_hex_keys_leave_the_editor_alone() {
    let editor = stack(&["workbench", "pane", "editor"]);
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for (keys, _) in hex_keys(os) {
            let got = run(&keymap, keys, &editor);
            assert!(
                !got.as_deref()
                    .is_some_and(|command| command.starts_with("hex.")),
                "{os:?} {keys}: {got:?}"
            );
            // Still bound there, to the editor's or the workbench's command.
            assert!(got.is_some(), "{os:?} {keys}");
        }
        let secondary = if os == Os::MacOs { "cmd" } else { "ctrl" };
        for (keys, command) in [
            (format!("{secondary}-f"), "find.show"),
            (format!("{secondary}-c"), "editor.copy"),
        ] {
            assert_eq!(
                run(&keymap, &keys, &editor).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
    }
}
