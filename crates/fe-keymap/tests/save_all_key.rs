//! The save keys: `cmd-alt-s` is Save All and `cmd-k s` Save without
//! Formatting on macOS, in the workbench; Linux binds neither, as VS Code's.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, KeySequence, Keymap, KeymapLayer, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn run(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Unmatched(Vec::new());
    for keystroke in KeySequence::parse_for(keys, os).unwrap().keystrokes() {
        outcome = matcher.push(*keystroke, stack);
    }
    if outcome == KeyOutcome::Pending {
        outcome = matcher.flush();
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn save_all_and_save_without_formatting_keys() {
    let stack = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let mac = keymap(Os::MacOs);
    assert_eq!(
        run(&mac, Os::MacOs, "cmd-alt-s", &stack).as_deref(),
        Some("workbench.save_all")
    );
    assert_eq!(
        run(&mac, Os::MacOs, "cmd-k s", &stack).as_deref(),
        Some("workbench.save_without_formatting")
    );
    let linux = keymap(Os::Linux);
    assert_eq!(run(&linux, Os::Linux, "ctrl-k s", &stack), None);
    assert!(linux.bindings().iter().all(|binding| !matches!(
        binding.command.as_str(),
        "workbench.save_all" | "workbench.save_without_formatting"
    )));
}
