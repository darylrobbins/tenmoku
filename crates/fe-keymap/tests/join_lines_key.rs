//! Join Lines' key: ctrl-j on macOS only, in the editor outside vim's
//! other modes.

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

/// What ctrl-j runs in `stack`: the last binding of exactly that key whose
/// context holds.
fn ctrl_j(keymap: &Keymap, os: Os, stack: &ContextStack) -> Option<String> {
    let keys = KeySequence::parse_for("ctrl-j", os).unwrap();
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
fn ctrl_j_joins_lines_on_macos_only() {
    let editor = stack(ContextFrame::new("editor"));
    let mac = keymap(Os::MacOs);
    assert_eq!(
        ctrl_j(&mac, Os::MacOs, &editor).as_deref(),
        Some("editor.join_lines")
    );
    for mode in ["insert", "replace"] {
        assert_eq!(
            ctrl_j(&mac, Os::MacOs, &vim(mode)).as_deref(),
            Some("editor.join_lines"),
            "vim {mode}"
        );
    }
    assert_eq!(ctrl_j(&mac, Os::MacOs, &vim("normal")), None);
    let linux = keymap(Os::Linux);
    assert_eq!(ctrl_j(&linux, Os::Linux, &editor), None);
}
