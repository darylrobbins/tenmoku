//! Organize Imports' and Refactor…'s keys, as VS Code binds them.

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

/// What `keys` runs in an editor: the last binding of exactly those keys
/// whose context holds.
fn resolves(keymap: &Keymap, os: Os, keys: &str, vim: Option<&str>) -> Option<String> {
    let mut editor = ContextFrame::new("editor");
    if let Some(mode) = vim {
        editor = editor.flag("vim.enabled").set("vim.mode", mode);
        if mode.starts_with("visual") {
            editor = editor.flag("vim.visual");
        }
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
fn organize_imports_and_refactor_keys_match_vs_code() {
    for (os, quick_fix) in [(Os::MacOs, "cmd-."), (Os::Linux, "ctrl-.")] {
        let keymap = keymap(os);
        for (keys, command) in [
            (quick_fix, "lsp.code_actions"),
            ("shift-alt-o", "lsp.organize_imports"),
            ("ctrl-shift-r", "lsp.refactor"),
        ] {
            for vim in [None, Some("normal"), Some("visual")] {
                assert_eq!(
                    resolves(&keymap, os, keys, vim).as_deref(),
                    Some(command),
                    "{keys} on {os:?}, vim {vim:?}"
                );
            }
        }
    }
}
