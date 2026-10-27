//! The picker chords: `secondary-k m` is Change Language Mode and
//! `secondary-k secondary-t` Change Color Theme, in the workbench.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, KeySequence, Keymap, KeymapLayer, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn run(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Unmatched(Vec::new());
    for keystroke in KeySequence::parse_for(keys, os).unwrap().keystrokes() {
        outcome = matcher.push(*keystroke, stack);
    }
    if outcome == KeyOutcome::Pending {
        outcome = matcher.flush();
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

fn stacks() -> [ContextStack; 2] {
    [
        ContextStack::new().with(ContextFrame::new("workbench")),
        ContextStack::new()
            .with(ContextFrame::new("workbench"))
            .with(ContextFrame::new("editor")),
    ]
}

#[test]
fn language_and_theme_pickers_have_chords() {
    for (os, language, theme) in [
        (Os::MacOs, "cmd-k m", "cmd-k cmd-t"),
        (Os::Linux, "ctrl-k m", "ctrl-k ctrl-t"),
    ] {
        let keymap = keymap(os);
        for stack in stacks() {
            assert_eq!(
                run(&keymap, os, language, &stack).as_deref(),
                Some("language.select"),
                "{os:?}"
            );
            assert_eq!(
                run(&keymap, os, theme, &stack).as_deref(),
                Some("theme.select"),
                "{os:?}"
            );
        }
    }
}

#[test]
fn lone_m_and_t_still_type_in_an_editor() {
    let stack = &stacks()[1];
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        assert_eq!(run(&keymap, os, "m", stack), None, "{os:?}");
        assert_eq!(run(&keymap, os, "t", stack), None, "{os:?}");
    }
}

#[test]
fn existing_chords_unchanged() {
    let stack = &stacks()[1];
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let (cmd, zero) = match os {
            Os::MacOs => ("cmd", "cmd"),
            _ => ("ctrl", "ctrl"),
        };
        for (keys, command) in [
            ("k z".to_owned(), "workbench.toggle_zen_mode"),
            ("k v".to_owned(), "markdown.show_preview_to_side"),
            (format!("k {cmd}-w"), "pane.close"),
            (format!("k {zero}-0"), "editor.fold_all"),
            (format!("k {cmd}-q"), "editor.go_to_last_edit_location"),
        ] {
            let keys = format!("{cmd}-{keys}");
            assert_eq!(
                run(&keymap, os, &keys, stack).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
    }
}
