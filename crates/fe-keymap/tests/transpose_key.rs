//! Transpose Letters key.

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
fn macos_binds_ctrl_t_in_the_editor_without_vim() {
    let mac = keymap(Os::MacOs);
    let editor = stack(ContextFrame::new("editor"));
    assert_eq!(
        resolve(&mac, Os::MacOs, "ctrl-t", &editor).as_deref(),
        Some("editor.transpose_letters")
    );
    // Vim insert and replace mode's ctrl-t indents the row: not rebound.
    for mode in ["insert", "replace", "normal"] {
        assert_eq!(
            resolve(&mac, Os::MacOs, "ctrl-t", &vim(mode)),
            None,
            "vim {mode}"
        );
    }
}

#[test]
fn linux_leaves_ctrl_t_to_vim_insert() {
    let linux = keymap(Os::Linux);
    assert!(
        linux
            .bindings()
            .iter()
            .all(|binding| binding.command.as_str() != "editor.transpose_letters")
    );
    assert_ne!(
        resolve(&linux, Os::Linux, "ctrl-t", &vim("insert")).as_deref(),
        Some("editor.transpose_letters")
    );
}
