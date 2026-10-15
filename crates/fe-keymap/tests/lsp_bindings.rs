//! Rename, code actions, formatting and signature help's keys, on each
//! platform, and that none of them takes a key from vim.

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
fn both_platforms() {
    let tables = [
        (
            Os::MacOs,
            [
                ("f2", "lsp.rename"),
                ("cmd-.", "lsp.code_actions"),
                ("shift-alt-f", "lsp.format_document"),
                ("cmd-k cmd-f", "lsp.format_selection"),
                ("cmd-shift-space", "editor.show_signature_help"),
                ("cmd-k cmd-i", "editor.show_hover"),
            ],
        ),
        (
            Os::Linux,
            [
                ("f2", "lsp.rename"),
                ("ctrl-.", "lsp.code_actions"),
                ("ctrl-shift-i", "lsp.format_document"),
                ("ctrl-k ctrl-f", "lsp.format_selection"),
                ("ctrl-shift-space", "editor.show_signature_help"),
                ("ctrl-k ctrl-i", "editor.show_hover"),
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
        }
    }
}

/// In every vim mode, each key vim binds still resolves to vim's binding.
#[test]
fn vim_keeps_its_keys() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for mode in [
            "normal",
            "insert",
            "replace",
            "operator_pending",
            "visual",
            "visual_line",
            "visual_block",
        ] {
            let stack = vim(mode);
            for binding in keymap.bindings() {
                if !binding.command.as_str().starts_with("vim.")
                    || binding.keys.keystrokes().len() != 1
                    || !binding.context.as_ref().is_none_or(|c| c.eval(&stack))
                {
                    continue;
                }
                let keystroke = binding.keys.keystrokes()[0];
                let command = keymap
                    .lookup(&keystroke, &stack)
                    .map(|invocation| invocation.command.as_str().to_owned());
                assert!(
                    command.as_deref().is_some_and(|c| c.starts_with("vim.")),
                    "{os:?} {mode} {keystroke}: {command:?}"
                );
            }
        }
    }
}

/// vim's key handler reads `ctrl-shift-i` as `<C-i>`, so formatting leaves it
/// to vim outside insert and replace mode.
#[test]
fn vim_reads_ctrl_shift_i() {
    for (os, keys) in [(Os::MacOs, "shift-alt-f"), (Os::Linux, "ctrl-shift-i")] {
        let keymap = keymap(os);
        for mode in [
            "normal",
            "operator_pending",
            "visual",
            "visual_line",
            "visual_block",
        ] {
            let command = resolves(&keymap, os, keys, &vim(mode));
            assert!(
                command.as_deref() != Some("lsp.format_document"),
                "{os:?} {mode} {keys}"
            );
        }
        for mode in ["insert", "replace"] {
            assert_eq!(
                resolves(&keymap, os, keys, &vim(mode)).as_deref(),
                Some("lsp.format_document"),
                "{os:?} {mode}"
            );
        }
    }
}
