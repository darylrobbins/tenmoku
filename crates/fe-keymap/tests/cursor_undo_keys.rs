//! Cursor Undo's key on each platform, and that vim keeps its own ctrl-u.

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

fn resolves(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
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
fn cursor_undo_key() {
    for (os, key) in [(Os::MacOs, "cmd-u"), (Os::Linux, "ctrl-u")] {
        let keymap = keymap(os);
        let editor = stack(ContextFrame::new("editor"));
        assert_eq!(
            resolves(&keymap, os, key, &editor).as_deref(),
            Some("editor.cursor_undo"),
            "{os:?}"
        );
        for mode in ["normal", "insert", "replace", "visual"] {
            assert_ne!(
                resolves(&keymap, os, key, &vim(mode)).as_deref(),
                Some("editor.cursor_undo"),
                "{os:?} {mode}"
            );
        }
    }
    let keymap = keymap(Os::Linux);
    assert_eq!(
        resolves(&keymap, Os::Linux, "ctrl-u", &vim("insert")).as_deref(),
        Some("vim.key")
    );
}
