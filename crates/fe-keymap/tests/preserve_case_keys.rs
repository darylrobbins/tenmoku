//! Preserve Case's key, VS Code's: alt-cmd-p on macOS and alt-p elsewhere,
//! in the find bar and in the search panel, and nowhere else.

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
    let find = stack(&["workbench", "pane", "find_bar", "text_input"]);
    let replace = stack(&[
        "workbench",
        "pane",
        "find_bar",
        "replace_input",
        "text_input",
    ]);
    let search = stack(&["workbench", "search_panel", "text_input"]);
    let editor = stack(&["workbench", "pane", "editor"]);
    for os in Os::ALL {
        let keys = match os {
            Os::MacOs => "alt-cmd-p",
            _ => "alt-p",
        };
        let keymap = keymap(os);
        for (stack, command) in [
            (&find, "find.toggle_preserve_case"),
            (&replace, "find.toggle_preserve_case"),
            (&search, "search.toggle_preserve_case"),
        ] {
            assert_eq!(
                run(&keymap, keys, stack).as_deref(),
                Some(command),
                "{os:?}"
            );
        }
        assert_eq!(run(&keymap, keys, &editor), None, "{os:?}");
    }
}
