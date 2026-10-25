//! Trim Trailing Whitespace's chord: `secondary-k secondary-x` in the editor.

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

fn stack(frame: ContextFrame) -> ContextStack {
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

/// What the chord runs in `stack`: the last binding of exactly that key
/// sequence whose context holds.
fn chord(keymap: &Keymap, os: Os, stack: &ContextStack) -> Option<String> {
    let keys = KeySequence::parse_for("secondary-k secondary-x", os).unwrap();
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
fn secondary_k_secondary_x_trims_in_the_editor() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        assert_eq!(
            chord(&keymap, os, &stack(ContextFrame::new("editor"))).as_deref(),
            Some("editor.trim_trailing_whitespace"),
            "{os:?}"
        );
        assert_eq!(chord(&keymap, os, &stack(ContextFrame::new("panel"))), None);
    }
}
