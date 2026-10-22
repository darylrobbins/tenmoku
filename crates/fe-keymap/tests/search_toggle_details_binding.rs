//! `search.toggle_details` resolves for each platform's key in the search
//! panel, and nowhere else.

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

fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(keys).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn resolves() {
    let panel = stack(&["workbench", "search_panel", "text_input"]);
    let editor = stack(&["workbench", "editor"]);
    for (os, keys) in [(Os::MacOs, "cmd-shift-j"), (Os::Linux, "ctrl-shift-j")] {
        let keymap = keymap(os);
        assert_eq!(
            run(&keymap, keys, &panel).as_deref(),
            Some("search.toggle_details"),
            "{os:?}"
        );
        assert_eq!(run(&keymap, keys, &editor), None, "{os:?}");
    }
}
