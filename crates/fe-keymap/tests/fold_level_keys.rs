//! Fold Level 1-7 and the recursive folds' keys: VS Code's secondary-k
//! chords in the editor, beside Fold All's and Unfold All's.

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
fn the_chords_fold_by_level_and_recursively() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let mut expected: Vec<_> = (1..=7)
            .map(|level| {
                (
                    format!("secondary-k secondary-{level}"),
                    format!("editor.fold_level_{level}"),
                )
            })
            .collect();
        expected.extend(
            [
                ("[", "editor.fold_recursively"),
                ("]", "editor.unfold_recursively"),
                ("0", "editor.fold_all"),
                ("j", "editor.unfold_all"),
            ]
            .map(|(key, command)| (format!("secondary-k secondary-{key}"), command.into())),
        );
        for (keys, expected) in expected {
            assert_eq!(
                command(&keymap, os, &keys).as_deref(),
                Some(expected.as_str()),
                "{keys} on {os:?}"
            );
        }
    }
}
