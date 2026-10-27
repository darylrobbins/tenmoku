//! The active file's path keys: `secondary-k p` copies its path and
//! `secondary-k r` reveals it, in the workbench, on each platform.

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
fn secondary_k_p_copies_and_secondary_k_r_reveals() {
    let stack = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let copy = run(&keymap, os, "secondary-k p", &stack);
        assert_eq!(
            copy.as_deref(),
            Some("workbench.copy_active_file_path"),
            "{os:?}"
        );
        let reveal = run(&keymap, os, "secondary-k r", &stack);
        assert_eq!(
            reveal.as_deref(),
            Some("workbench.reveal_active_file_in_os"),
            "{os:?}"
        );
    }
}
