//! VS Code's keyboard column selection: cmd-shift-alt-arrow on macOS only,
//! where ctrl-shift-alt-down elsewhere stays Duplicate Rows.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// What typing `key` runs in the editor, if anything.
fn run(keymap: &Keymap, key: &str) -> Option<String> {
    let stack: ContextStack = [ContextFrame::new("workbench"), ContextFrame::new("editor")]
        .into_iter()
        .collect();
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(key).unwrap(), &stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn column_select_keys_resolve_on_macos_only() {
    for (sources, os) in [
        ([DEFAULT_MACOS, DEFAULT], Os::MacOs),
        ([DEFAULT_LINUX, DEFAULT], Os::Linux),
    ] {
        for source in sources {
            let keymap = keymap(source, os);
            for direction in ["left", "right", "up", "down"] {
                let expected =
                    (os == Os::MacOs).then(|| format!("editor.column_select_{direction}"));
                assert_eq!(
                    run(&keymap, &format!("cmd-shift-alt-{direction}")),
                    expected,
                    "{os:?} {direction}"
                );
            }
            if os != Os::MacOs {
                assert_eq!(
                    run(&keymap, "ctrl-shift-alt-down").as_deref(),
                    Some("editor.duplicate_rows")
                );
            }
        }
    }
}
