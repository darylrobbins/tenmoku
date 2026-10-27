//! Move Last Selection to Next Find Match's key: VS Code's secondary-k
//! secondary-d chord in the editor, not in vim normal mode.

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

/// What `keys` runs in a focused editor, with vim `vim` or not: the last
/// binding of exactly those keys whose context holds.
fn command(keymap: &Keymap, os: Os, keys: &str, vim: bool) -> Option<String> {
    let mut editor = ContextFrame::new("editor");
    if vim {
        editor = editor.flag("vim.enabled").set("vim.mode", "normal");
    }
    let stack: ContextStack = [ContextFrame::new("workbench"), editor]
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
fn the_chord_moves_the_last_selection() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        assert_eq!(
            command(&keymap, os, "secondary-k secondary-d", false).as_deref(),
            Some("editor.move_last_selection_to_next_find_match"),
            "{os:?}"
        );
        assert_eq!(
            command(&keymap, os, "secondary-d", false).as_deref(),
            Some("editor.select_next_occurrence"),
            "{os:?}"
        );
        assert_eq!(
            command(&keymap, os, "secondary-k secondary-d", true),
            None,
            "{os:?}"
        );
    }
}
