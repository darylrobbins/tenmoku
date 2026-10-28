//! Inline Edit with AI's key and the input's own, on each platform, and that
//! vim keeps `ctrl-i`.

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

fn stack(frames: impl IntoIterator<Item = ContextFrame>) -> ContextStack {
    [ContextFrame::new("workbench")]
        .into_iter()
        .chain(frames)
        .collect()
}

fn vim(mode: &str) -> ContextStack {
    stack([ContextFrame::new("editor")
        .flag("vim.enabled")
        .set("vim.mode", mode)])
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
fn secondary_i_opens_the_input_in_an_editor() {
    for (os, keys) in [(Os::MacOs, "cmd-i"), (Os::Linux, "ctrl-i")] {
        let keymap = keymap(os);
        let editor = stack([ContextFrame::new("editor")]);
        assert_eq!(
            resolves(&keymap, os, keys, &editor).as_deref(),
            Some("ai.inline_edit"),
            "{os:?}"
        );
    }
}

/// With vim on, `ctrl-i` is `<C-i>` off macOS: no binding outside insert
/// and replace mode, which keep the editor's keys.
#[test]
fn vim_keeps_ctrl_i() {
    for (os, keys) in [(Os::MacOs, "cmd-i"), (Os::Linux, "ctrl-i")] {
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
                command.as_deref() != Some("ai.inline_edit"),
                "{os:?} {mode}"
            );
        }
        for mode in ["insert", "replace"] {
            assert_eq!(
                resolves(&keymap, os, keys, &vim(mode)).as_deref(),
                Some("ai.inline_edit"),
                "{os:?} {mode}"
            );
        }
    }
}

#[test]
fn the_input_takes_enter_and_escape() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let input = stack([
            ContextFrame::new("editor"),
            ContextFrame::new("inline_edit"),
            ContextFrame::new("text_input"),
        ]);
        assert_eq!(
            resolves(&keymap, os, "enter", &input).as_deref(),
            Some("inline_edit.submit")
        );
        assert_eq!(
            resolves(&keymap, os, "escape", &input).as_deref(),
            Some("inline_edit.cancel")
        );
    }
}
