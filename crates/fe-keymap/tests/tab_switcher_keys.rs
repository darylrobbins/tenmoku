//! The most-recently-used tab switcher's keys resolve on each platform, in
//! a workbench editor but not in a terminal, beside the positional tab keys.

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
fn ctrl_tab_keys_resolve() {
    let editor = stack(&["workbench", "editor"]);
    let list = stack(&["workbench", "tab_switcher"]);
    let terminal = stack(&["workbench", "terminal"]);
    for (os, next, previous) in [
        (Os::MacOs, "cmd-shift-]", "cmd-shift-["),
        (Os::Linux, "ctrl-pagedown", "ctrl-pageup"),
    ] {
        let keymap = keymap(os);
        for frames in [&editor, &list] {
            assert_eq!(
                run(&keymap, "ctrl-tab", frames).as_deref(),
                Some("workbench.tab_switcher_next"),
                "{os:?}"
            );
            assert_eq!(
                run(&keymap, "ctrl-shift-tab", frames).as_deref(),
                Some("workbench.tab_switcher_previous"),
                "{os:?}"
            );
        }
        for keys in ["ctrl-tab", "ctrl-shift-tab"] {
            assert_eq!(run(&keymap, keys, &terminal), None, "{os:?} {keys}");
        }
        assert_eq!(
            run(&keymap, next, &editor).as_deref(),
            Some("workbench.next_tab"),
            "{os:?}"
        );
        assert_eq!(
            run(&keymap, previous, &editor).as_deref(),
            Some("workbench.previous_tab"),
            "{os:?}"
        );
    }
}
