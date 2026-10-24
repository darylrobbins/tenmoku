//! The AI Changes panel's file keys resolve on each platform, only with the
//! panel's context, and are no other command's.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, Keymap, KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[&str]) -> ContextStack {
    frames.iter().map(|name| ContextFrame::new(*name)).collect()
}

/// What pressing `keys` runs in `stack`, if anything.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(keys).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn binding_resolves_per_platform() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let panel = stack(&["workbench", "ai_changes"]);
        assert_eq!(
            run(&keymap, "alt-]", &panel).as_deref(),
            Some("ai.changes_next_file"),
            "{os:?}"
        );
        assert_eq!(
            run(&keymap, "alt-[", &panel).as_deref(),
            Some("ai.changes_previous_file"),
            "{os:?}"
        );
        // Only the panel's: not in an editor.
        let editor = stack(&["workbench", "editor"]);
        assert_eq!(run(&keymap, "alt-]", &editor), None, "{os:?}");
        assert_eq!(run(&keymap, "alt-[", &editor), None, "{os:?}");
    }
}
