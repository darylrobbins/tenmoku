//! Compare Active File with Saved: `cmd-k d` on macOS and `ctrl-k d` on
//! Linux, in the editor context; the move-last-selection chord
//! `cmd-k cmd-d` is not affected.

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

fn run(keymap: &Keymap, keys: &[&str], stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = None;
    for key in keys {
        outcome = Some(matcher.push(Keystroke::parse(key).unwrap(), stack));
    }
    match outcome? {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn the_chord_resolves_in_the_editor() {
    let editor: ContextStack = ["workbench", "editor"]
        .map(ContextFrame::new)
        .into_iter()
        .collect();
    for (os, secondary) in [(Os::MacOs, "cmd"), (Os::Linux, "ctrl")] {
        let keymap = keymap(os);
        let k = format!("{secondary}-k");
        assert_eq!(
            run(&keymap, &[&k, "d"], &editor).as_deref(),
            Some("diff.compare_saved"),
            "{os:?}"
        );
        let d = format!("{secondary}-d");
        assert_eq!(
            run(&keymap, &[&k, &d], &editor).as_deref(),
            Some("editor.move_last_selection_to_next_find_match"),
            "{os:?}"
        );
    }
}
