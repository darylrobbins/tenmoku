//! `secondary-shift-\` is Go to Bracket in the editor, and `secondary-\` is
//! still the split.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, Keymap, KeymapLayer, Keystroke};
use kiln_core::Os;

fn resolve(os: Os, keys: &str) -> Option<String> {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    let keymap = Keymap::from_layers(vec![layer.unwrap()]);
    let stack: ContextStack = [ContextFrame::new("workbench"), ContextFrame::new("editor")]
        .into_iter()
        .collect();
    let keystroke = Keystroke::parse(keys).unwrap();
    keymap
        .lookup(&keystroke, &stack)
        .map(|invocation| invocation.command.as_str().to_owned())
}

#[test]
fn go_to_bracket_key() {
    for (os, shifted, plain) in [
        (Os::MacOs, "cmd-shift-\\", "cmd-\\"),
        (Os::Linux, "ctrl-shift-\\", "ctrl-\\"),
    ] {
        assert_eq!(
            resolve(os, shifted).as_deref(),
            Some("editor.go_to_bracket"),
            "{os:?}"
        );
        assert_eq!(
            resolve(os, plain).as_deref(),
            Some("pane.split_right"),
            "{os:?}"
        );
    }
}
