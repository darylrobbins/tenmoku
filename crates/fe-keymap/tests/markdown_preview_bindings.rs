//! The Markdown preview's keys, on each platform: in the editor, with vim
//! off or on, but never in the terminal, which pastes on ctrl-shift-v.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, KeySequence, Keymap, KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench with an editor focused, in vim `mode` if any.
fn editor(vim: Option<&str>) -> ContextStack {
    let mut editor = ContextFrame::new("editor");
    if let Some(mode) = vim {
        editor = editor.flag("vim.enabled").set("vim.mode", mode);
        if mode.starts_with("visual") {
            editor = editor.flag("vim.visual");
        }
    }
    [ContextFrame::new("workbench"), editor]
        .into_iter()
        .collect()
}

/// What typing `keys`, one keystroke or a chord, runs in `stack`.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Pending;
    for key in keys.split(' ') {
        outcome = matcher.push(Keystroke::parse(key).unwrap(), stack);
        if outcome != KeyOutcome::Pending {
            break;
        }
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn markdown_preview_keys_are_bound_on_each_platform() {
    let terminal: ContextStack = ["workbench", "terminal"]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    for os in Os::ALL {
        let (preview, to_side) = match os {
            Os::MacOs => ("cmd-shift-v", "cmd-k v"),
            Os::Linux | Os::Windows => ("ctrl-shift-v", "ctrl-k v"),
        };
        let keymap = keymap(os);
        let table = [
            (preview, "markdown.show_preview"),
            (to_side, "markdown.show_preview_to_side"),
        ];
        for (keys, command) in table {
            for vim in [None, Some("normal"), Some("insert"), Some("visual")] {
                assert_eq!(
                    run(&keymap, keys, &editor(vim)).as_deref(),
                    Some(command),
                    "{os:?} {vim:?} {keys}"
                );
            }
            let got = run(&keymap, keys, &terminal);
            assert_ne!(got.as_deref(), Some(command), "{os:?} terminal {keys}");

            // Neither these keys nor a chord starting with them run anything
            // else.
            let sequence = KeySequence::parse_for(keys, os).unwrap();
            let users: Vec<_> = (keymap.bindings().iter())
                .filter(|binding| (binding.keys.keystrokes()).starts_with(sequence.keystrokes()))
                .map(|binding| binding.command.as_str())
                .collect();
            assert_eq!(users, [command], "{os:?} {keys}");
        }
    }
}
