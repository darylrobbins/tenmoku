//! Delete All Left, Delete All Right and Delete Word Right's keys.

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

/// What `keys` runs in `stack`: the last binding of exactly that key whose
/// context holds.
fn resolve(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
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
fn macos_binds_the_three_in_the_editor_outside_vim_normal() {
    let mac = keymap(Os::MacOs);
    let editor = stack(ContextFrame::new("editor"));
    for (keys, command) in [
        ("cmd-backspace", "editor.delete_all_left"),
        ("ctrl-k", "editor.delete_all_right"),
        ("alt-delete", "editor.delete_word_right"),
    ] {
        assert_eq!(
            resolve(&mac, Os::MacOs, keys, &editor).as_deref(),
            Some(command),
            "{keys}"
        );
    }
    for mode in ["insert", "replace"] {
        for keys in ["cmd-backspace", "ctrl-k", "alt-delete"] {
            assert!(
                resolve(&mac, Os::MacOs, keys, &vim(mode)).is_some(),
                "{keys} in vim {mode}"
            );
        }
    }
    for keys in ["cmd-backspace", "ctrl-k", "alt-delete"] {
        assert_eq!(
            resolve(&mac, Os::MacOs, keys, &vim("normal")),
            None,
            "{keys}"
        );
    }
}

#[test]
fn a_proposal_keeps_cmd_backspace() {
    let mac = keymap(Os::MacOs);
    let proposal = stack(ContextFrame::new("editor").flag("proposal"));
    assert_eq!(
        resolve(&mac, Os::MacOs, "cmd-backspace", &proposal).as_deref(),
        Some("proposal.reject_all")
    );
}

#[test]
fn linux_binds_only_ctrl_delete() {
    let linux = keymap(Os::Linux);
    let editor = stack(ContextFrame::new("editor"));
    for (mode, bound) in [("insert", true), ("replace", true), ("normal", false)] {
        assert_eq!(
            resolve(&linux, Os::Linux, "ctrl-delete", &vim(mode)).is_some(),
            bound,
            "vim {mode}"
        );
    }
    assert_eq!(
        resolve(&linux, Os::Linux, "ctrl-delete", &editor).as_deref(),
        Some("editor.delete_word_right")
    );
    for command in ["editor.delete_all_left", "editor.delete_all_right"] {
        assert!(
            linux
                .bindings()
                .iter()
                .all(|binding| binding.command.as_str() != command),
            "{command}"
        );
    }
}
