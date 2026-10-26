//! Toggle Block Comment's key: shift-alt-a, in the editor outside vim's
//! normal mode.

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

fn vim(mode: &str) -> ContextStack {
    stack(
        ContextFrame::new("editor")
            .flag("vim.enabled")
            .set("vim.mode", mode),
    )
}

/// What shift-alt-a runs in `stack`: the last binding of exactly that key whose
/// context holds.
fn key(keymap: &Keymap, os: Os, stack: &ContextStack) -> Option<String> {
    let keys = KeySequence::parse_for("shift-alt-a", os).unwrap();
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
fn shift_alt_a_toggles_block_comment() {
    for os in [Os::MacOs, Os::Linux] {
        let map = keymap(os);
        let editor = stack(ContextFrame::new("editor"));
        let want = Some("editor.toggle_block_comment");
        assert_eq!(key(&map, os, &editor).as_deref(), want, "{os:?}");
        for mode in ["insert", "replace"] {
            assert_eq!(key(&map, os, &vim(mode)).as_deref(), want, "{os:?} {mode}");
        }
        assert_eq!(key(&map, os, &vim("normal")), None, "{os:?}");
    }
}
