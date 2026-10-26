//! Go to Last Edit Location's key: VS Code's secondary-k secondary-q chord
//! in the editor, beside secondary-q alone quitting.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, KeySequence, Keymap, KeymapLayer};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// What `keys` runs in a focused editor: the last binding of exactly those
/// keys whose context holds.
fn command(keymap: &Keymap, os: Os, keys: &str) -> Option<String> {
    let stack: ContextStack = [ContextFrame::new("workbench"), ContextFrame::new("editor")]
        .into_iter()
        .collect();
    let keys = KeySequence::parse_for(keys, os).unwrap();
    keymap
        .bindings()
        .iter()
        .rev()
        .find(|binding| {
            binding.keys == keys && binding.context.as_ref().is_none_or(|c| c.eval(&stack))
        })
        .map(|binding| binding.command.as_str().to_owned())
}

#[test]
fn the_chord_goes_to_the_last_edit_location() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for (keys, expected) in [
            ("secondary-k secondary-q", "editor.go_to_last_edit_location"),
            ("secondary-q", "app.quit"),
        ] {
            assert_eq!(
                command(&keymap, os, keys).as_deref(),
                Some(expected),
                "{keys} on {os:?}"
            );
        }
    }
}
