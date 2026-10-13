//! The terminal's find key, and the find row's keys, resolve on each
//! platform.

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
fn resolves() {
    let terminal = stack(&["workbench", "terminal"]);
    let editor = stack(&["workbench", "editor"]);
    let row = stack(&["workbench", "find_bar", "text_input"]);

    let macos = keymap(Os::MacOs);
    assert_eq!(
        run(&macos, "cmd-f", &terminal).as_deref(),
        Some("terminal.find")
    );

    let linux = keymap(Os::Linux);
    assert_eq!(
        run(&linux, "ctrl-shift-f", &terminal).as_deref(),
        Some("terminal.find")
    );
    assert_eq!(
        run(&linux, "ctrl-shift-f", &editor).as_deref(),
        Some("search.show")
    );
    assert_eq!(run(&linux, "ctrl-f", &terminal), None);

    for keymap in [&macos, &linux] {
        for (keys, command) in [
            ("enter", "find.next"),
            ("shift-enter", "find.previous"),
            ("escape", "find.close"),
        ] {
            assert_eq!(run(keymap, keys, &row).as_deref(), Some(command), "{keys}");
        }
    }
}
