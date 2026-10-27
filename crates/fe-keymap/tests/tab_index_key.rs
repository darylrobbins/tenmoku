//! ctrl-1 to ctrl-9 (alt-1 to alt-9 off macOS) open the tab at that index
//! in the workbench, a focused terminal keeps them, and the digit keys
//! bound elsewhere are unchanged.

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
fn digits_open_the_tab_at_that_index() {
    for (os, modifier) in [(Os::MacOs, "ctrl"), (Os::Linux, "alt")] {
        let keymap = keymap(os);
        for n in 1..=9 {
            assert_eq!(
                run(
                    &keymap,
                    &format!("{modifier}-{n}"),
                    &stack(&["workbench", "editor"])
                )
                .as_deref(),
                Some(format!("workbench.open_editor_at_index{n}").as_str()),
                "{os:?} {n}"
            );
        }
    }
}

#[test]
fn a_focused_terminal_keeps_its_digit_keys() {
    for (os, modifier) in [(Os::MacOs, "ctrl"), (Os::Linux, "alt")] {
        let keymap = keymap(os);
        let terminal = stack(&["workbench", "terminal"]);
        for n in 1..=9 {
            let command = run(&keymap, &format!("{modifier}-{n}"), &terminal);
            assert!(
                !command.is_some_and(|command| command.starts_with("workbench.open_editor")),
                "{os:?} {n}"
            );
        }
    }
}

#[test]
fn the_image_viewer_zoom_keys_are_unchanged() {
    for (os, keys) in [(Os::MacOs, "cmd-1"), (Os::Linux, "ctrl-1")] {
        let command = run(&keymap(os), keys, &stack(&["workbench", "ImageViewer"]));
        assert!(
            command.is_some_and(|command| !command.starts_with("workbench.open_editor")),
            "{os:?}"
        );
    }
}
