//! The chat's keys and its View menu item resolve on each platform, and the
//! composer's keys beat the editor's own.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    Binding, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapIssue, KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}: no conflict or other problem");
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
fn chat_keys_and_menu() {
    // The stack a key in the composer has: the composer is an editor inside
    // `chat_composer`.
    let composer = stack(&["workbench", "chat", "chat_composer", "editor"]);
    let editor = stack(&["workbench", "editor"]);
    let workbench = stack(&["workbench"]);

    for (os, source, open) in [
        (Os::MacOs, DEFAULT_MACOS, "ctrl-cmd-i"),
        (Os::Linux, DEFAULT_LINUX, "ctrl-alt-i"),
    ] {
        let keymap = keymap(source, os);
        let open_keys = KeySequence::parse(open).unwrap();
        let got = |keys, stack: &ContextStack| run(&keymap, keys, stack);
        assert_eq!(
            got(open, &workbench).as_deref(),
            Some("chat.open"),
            "{os:?}"
        );
        assert_eq!(
            got("enter", &composer).as_deref(),
            Some("chat.send"),
            "{os:?}"
        );
        assert_eq!(
            got("shift-enter", &composer).as_deref(),
            Some("editor.newline"),
            "{os:?}"
        );
        assert_eq!(
            got("escape", &composer).as_deref(),
            Some("chat.stop"),
            "{os:?}"
        );
        // Elsewhere the editor's keys are its own.
        assert_eq!(
            got("enter", &editor).as_deref(),
            Some("editor.newline"),
            "{os:?}"
        );
        assert_eq!(
            got("escape", &editor).as_deref(),
            Some("editor.single_selection"),
            "{os:?}"
        );
        assert_eq!(got("shift-enter", &editor), None, "{os:?}");

        // The keymap's own conflict report, in the contexts the chat's keys
        // hold in, has nothing for any of the four bindings: found by their
        // keys and contexts, not by name.
        let mine = |binding: &Binding| {
            let context = binding.context.as_ref().map(ToString::to_string);
            context.as_deref() == Some("chat_composer && editor")
                || (context.as_deref() == Some("workbench") && binding.keys == open_keys)
        };
        let report = keymap.report_with(&[workbench.clone(), composer.clone(), editor.clone()]);
        for issue in &report {
            let involved: Vec<&Binding> = match issue {
                KeymapIssue::Overridden { winner, loser } => vec![winner, loser],
                KeymapIssue::Shadowed { winner, loser, .. } => vec![winner, loser],
                KeymapIssue::PrefixDelay { prefix, longer, .. } => vec![prefix, longer],
            };
            assert!(!involved.into_iter().any(mine), "{os:?}: {issue}");
        }
        assert_eq!(
            keymap.bindings().iter().filter(|b| mine(b)).count(),
            4,
            "{os:?}"
        );

        let (bar, diagnostics) = match os {
            Os::MacOs => fe_keymap::default_menus_macos(None),
            _ => fe_keymap::default_menus_linux(None),
        };
        assert_eq!(diagnostics, []);
        let bar = bar.unwrap();
        let view = bar.menu("View").unwrap();
        let ids: Vec<_> = view.items.iter().filter_map(|item| item.id()).collect();
        let terminal = ids.iter().position(|id| *id == "terminal.toggle").unwrap();
        assert_eq!(ids.get(terminal + 1), Some(&"chat.open"), "{os:?}");
        let shortcut = view
            .item("chat.open")
            .unwrap()
            .command()
            .unwrap()
            .shortcut(&keymap);
        assert_eq!(shortcut, Some(KeySequence::parse(open).unwrap()), "{os:?}");
    }
}
