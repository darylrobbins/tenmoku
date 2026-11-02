//! Expand and shrink selection's keys in vim's normal and visual modes.

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

fn vim(mode: &str) -> ContextStack {
    let mut frame = ContextFrame::new("editor")
        .flag("vim.enabled")
        .set("vim.mode", mode);
    if mode.starts_with("visual") {
        frame = frame.flag("vim.visual");
    }
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
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
fn vim_expand_and_shrink_keys_in_normal_and_visual() {
    let tables = [
        (
            Os::MacOs,
            [
                ("ctrl-shift-cmd-right", "vim.expand_selection"),
                ("ctrl-shift-cmd-left", "vim.shrink_selection"),
            ],
        ),
        (
            Os::Linux,
            [
                ("shift-alt-right", "vim.expand_selection"),
                ("shift-alt-left", "vim.shrink_selection"),
            ],
        ),
    ];
    for (os, table) in tables {
        let keymap = keymap(os);
        for (keys, command) in table {
            for mode in ["normal", "visual", "visual_line", "visual_block"] {
                assert_eq!(
                    resolves(&keymap, os, keys, &vim(mode)).as_deref(),
                    Some(command),
                    "{os:?} {keys} in vim {mode}"
                );
            }
            for mode in ["insert", "replace", "operator_pending", "cmdline"] {
                assert_eq!(
                    resolves(&keymap, os, keys, &vim(mode)),
                    None,
                    "{os:?} {keys} in vim {mode}"
                );
            }
        }
    }
}
