//! Go to Implementations' key, cmd-f12 on macOS and ctrl-f12 elsewhere.

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
fn goto_implementation_is_cmd_f12_on_macos_and_ctrl_f12_elsewhere() {
    for (os, keys) in [(Os::MacOs, "cmd-f12"), (Os::Linux, "ctrl-f12")] {
        let keymap = keymap(os);
        for stack in [editor(), vim("normal")] {
            assert_eq!(
                resolves(&keymap, os, keys, &stack).as_deref(),
                Some("nav.goto_implementation"),
                "{os:?} {keys}"
            );
        }
    }
    // Neither platform's other key is bound.
    assert_eq!(
        resolves(&keymap(Os::MacOs), Os::MacOs, "ctrl-f12", &editor()),
        None
    );
    assert_eq!(
        resolves(&keymap(Os::Linux), Os::Linux, "cmd-f12", &editor()),
        None
    );
}
