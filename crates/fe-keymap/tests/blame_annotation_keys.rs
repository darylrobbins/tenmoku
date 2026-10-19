//! Git: Toggle Blame Annotations' key, the git::Blame chord, and its View
//! menu item: cmd-alt-g b on macOS; ctrl-alt-g b elsewhere, kept out of
//! vim's normal mode, where the editor reads ctrl-alt-g as vim's <C-g>.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

const COMMAND: &str = "git.toggle_blame_annotations";

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench, then an editor: vim off, or on in `mode`.
fn editor(vim: Option<&str>) -> ContextStack {
    let frame = ContextFrame::new("editor");
    let frame = match vim {
        None => frame,
        Some(mode) => frame.flag("vim.enabled").set("vim.mode", mode),
    };
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

/// The command `keys` run in `stack`, pressed one after another, if any.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let keys = KeySequence::parse(keys).unwrap();
    let (last, first) = keys.keystrokes().split_last().unwrap();
    for keystroke in first {
        if matcher.push(*keystroke, stack) != KeyOutcome::Pending {
            return None;
        }
    }
    match matcher.push(*last, stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn blame_annotation_keys_toggle_the_column() {
    let platforms = [
        (Os::MacOs, DEFAULT_MACOS, "cmd-alt-g b"),
        (Os::Linux, DEFAULT_LINUX, "ctrl-alt-g b"),
    ];
    for (os, generated, keys) in platforms {
        let mac = os == Os::MacOs;
        for keymap in [keymap(generated, os), keymap(DEFAULT, os)] {
            for vim in [None, Some("insert")] {
                let got = run(&keymap, keys, &editor(vim));
                assert_eq!(got.as_deref(), Some(COMMAND), "{os:?} {keys} {vim:?}");
            }
            // On macOS no vim mode reads cmd chords; elsewhere normal mode
            // keeps ctrl-alt-g.
            let normal = run(&keymap, keys, &editor(Some("normal")));
            let expected = mac.then_some(COMMAND);
            assert_eq!(normal.as_deref(), expected, "{os:?} {keys} in normal mode");
            // The other platform's chord does nothing.
            let other = if mac { "ctrl-alt-g b" } else { "cmd-alt-g b" };
            assert_eq!(run(&keymap, other, &editor(None)), None, "{os:?} {other}");
            // Only in an editor.
            let workbench: ContextStack = [ContextFrame::new("workbench")].into_iter().collect();
            assert_eq!(run(&keymap, keys, &workbench), None, "{os:?} {keys}");
        }
        let normal = run(
            &keymap(generated, os),
            "ctrl-alt-g b",
            &editor(Some("normal")),
        );
        assert_eq!(normal, None, "{os:?} ctrl-alt-g b in normal mode");

        // The View menu's item, beside Toggle Line Blame.
        let (bar, diagnostics) = match os {
            Os::MacOs => fe_keymap::default_menus_macos(None),
            _ => fe_keymap::default_menus_linux(None),
        };
        assert_eq!(diagnostics, []);
        let bar = bar.unwrap();
        let view = bar.menu("View").expect("a View menu");
        let item = (view.item(COMMAND))
            .and_then(|item| item.command())
            .unwrap_or_else(|| panic!("View > {COMMAND}"));
        assert_eq!(item.title, "Toggle Blame Annotations");
        let ids: Vec<&str> = (view.items.iter()).filter_map(|item| item.id()).collect();
        let at = ids.iter().position(|id| *id == COMMAND).unwrap();
        assert_eq!(ids[at - 1], "git.toggle_line_blame", "{ids:?}");
    }
}
