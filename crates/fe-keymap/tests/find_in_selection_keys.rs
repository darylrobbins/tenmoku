//! Find in Selection's key, VS Code's: alt-cmd-l on macOS and alt-l elsewhere,
//! in the find bar, its replace input too, and nowhere else.

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
fn resolves_in_the_find_bar() {
    let find = stack(&["workbench", "pane", "find_bar", "text_input"]);
    let replace = stack(&[
        "workbench",
        "pane",
        "find_bar",
        "replace_input",
        "text_input",
    ]);
    let editor = stack(&["workbench", "pane", "editor"]);
    for os in Os::ALL {
        let keys = match os {
            Os::MacOs => "alt-cmd-l",
            _ => "alt-l",
        };
        let keymap = keymap(os);
        for stack in [&find, &replace] {
            assert_eq!(
                run(&keymap, keys, stack).as_deref(),
                Some("find.toggle_in_selection"),
                "{os:?}"
            );
        }
        assert_eq!(run(&keymap, keys, &editor), None, "{os:?}");
    }
}
