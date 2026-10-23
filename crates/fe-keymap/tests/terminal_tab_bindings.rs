//! The terminal's toggle, new and cycle keys resolve on each platform, and
//! the cycle keys only win with a terminal focused.

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
fn ctrl_backtick_toggles_in_editor_and_terminal() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for frames in [["workbench", "editor"], ["workbench", "terminal"]] {
            assert_eq!(
                run(&keymap, "ctrl-`", &stack(&frames)).as_deref(),
                Some("terminal.toggle"),
                "{os:?} {frames:?}"
            );
        }
    }
}

#[test]
fn ctrl_shift_backtick_creates_a_terminal() {
    for (os, keys) in [(Os::MacOs, "ctrl-shift-`"), (Os::Linux, "ctrl-~")] {
        let keymap = keymap(os);
        for frames in [["workbench", "editor"], ["workbench", "terminal"]] {
            assert_eq!(
                run(&keymap, keys, &stack(&frames)).as_deref(),
                Some("terminal.new"),
                "{os:?} {frames:?}"
            );
        }
    }
}

#[test]
fn terminal_cycle_keys_beat_editor_tabs() {
    let terminal = stack(&["workbench", "terminal"]);
    let editor = stack(&["workbench", "editor"]);
    for (os, next, previous) in [
        (Os::MacOs, "cmd-shift-]", "cmd-shift-["),
        (Os::Linux, "ctrl-pagedown", "ctrl-pageup"),
    ] {
        let keymap = keymap(os);
        assert_eq!(
            run(&keymap, next, &terminal).as_deref(),
            Some("terminal.next")
        );
        assert_eq!(
            run(&keymap, previous, &terminal).as_deref(),
            Some("terminal.previous")
        );
        assert_eq!(
            run(&keymap, next, &editor).as_deref(),
            Some("workbench.next_tab")
        );
        assert_eq!(
            run(&keymap, previous, &editor).as_deref(),
            Some("workbench.previous_tab")
        );
    }
}
