//! Expand and shrink selection's keys, on each platform, and that vim keeps
//! them to itself.

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

fn editor() -> ContextStack {
    stack(ContextFrame::new("editor"))
}

fn vim(mode: &str) -> ContextStack {
    let mut frame = ContextFrame::new("editor")
        .flag("vim.enabled")
        .set("vim.mode", mode);
    if mode.starts_with("visual") {
        frame = frame.flag("vim.visual");
    }
    stack(frame)
}

/// What `keys`, one keystroke or a chord, runs in `stack`: the last binding
/// of exactly those keys whose context holds.
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
fn expand_and_shrink_selection_keys_match_vs_code() {
    let tables = [
        (
            Os::MacOs,
            [
                ("ctrl-shift-cmd-right", "editor.expand_selection"),
                ("ctrl-shift-cmd-left", "editor.shrink_selection"),
            ],
        ),
        (
            Os::Linux,
            [
                ("shift-alt-right", "editor.expand_selection"),
                ("shift-alt-left", "editor.shrink_selection"),
            ],
        ),
    ];
    for (os, table) in tables {
        let keymap = keymap(os);
        for (keys, command) in table {
            assert_eq!(
                resolves(&keymap, os, keys, &editor()).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
            for mode in [
                "normal",
                "insert",
                "replace",
                "operator_pending",
                "visual",
                "visual_line",
                "visual_block",
            ] {
                assert_eq!(
                    resolves(&keymap, os, keys, &vim(mode)),
                    None,
                    "{os:?} {keys} in vim {mode}"
                );
            }
        }
    }
}
