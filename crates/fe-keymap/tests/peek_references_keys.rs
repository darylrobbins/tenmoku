//! Peek References' keys, as VS Code binds them: `shift-f12` peeks at the
//! references in the editor, and Find All References moves to
//! `shift-alt-f12`, anywhere in the workbench but the terminal, which keeps
//! both keys for the shell. In the peek, its list is a References panel:
//! up and down select, enter opens, and escape closes the peek.

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

/// The workbench and `frames` in it.
fn stack(frames: &[ContextFrame]) -> ContextStack {
    let mut stack = ContextStack::new().with(ContextFrame::new("workbench"));
    for frame in frames {
        stack.push(frame.clone());
    }
    stack
}

/// An editor's frame, with vim on in `mode` or off.
fn editor(vim: Option<&str>) -> ContextFrame {
    let frame = ContextFrame::new("editor");
    match vim {
        Some(mode) => frame.flag("vim.enabled").set("vim.mode", mode),
        None => frame,
    }
}

/// What pressing `keys` runs in `stack`.
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

#[test]
fn shift_f12_peeks_and_shift_alt_f12_finds_all() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for vim in [None, Some("normal")] {
            let editor = stack(&[editor(vim)]);
            let peeked = run(&keymap, os, "shift-f12", &editor);
            assert_eq!(
                peeked.as_deref(),
                Some("editor.peek_references"),
                "{os:?} {vim:?}"
            );
            let found = run(&keymap, os, "shift-alt-f12", &editor);
            assert_eq!(
                found.as_deref(),
                Some("nav.find_references"),
                "{os:?} {vim:?}"
            );
        }
        // Find All References outside an editor too; no peek there.
        let panel = stack(&[ContextFrame::new("project_panel")]);
        let found = run(&keymap, os, "shift-alt-f12", &panel);
        assert_eq!(found.as_deref(), Some("nav.find_references"), "{os:?}");
        assert_eq!(run(&keymap, os, "shift-f12", &panel), None, "{os:?}");
        // vim's `g r r` stays Find All References.
        let normal = stack(&[editor(Some("normal"))]);
        let found = run(&keymap, os, "g r r", &normal);
        assert_eq!(found.as_deref(), Some("nav.find_references"), "{os:?}");
    }
}

#[test]
fn the_terminal_keeps_both_keys() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let terminal = stack(&[ContextFrame::new("terminal")]);
        for keys in ["shift-f12", "shift-alt-f12"] {
            assert_eq!(run(&keymap, os, keys, &terminal), None, "{os:?} {keys}");
        }
    }
}

#[test]
fn keys_in_the_peek_list() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let list = stack(&[
            ContextFrame::new("peek"),
            ContextFrame::new("ReferencesPanel"),
        ]);
        for (keys, command) in [
            ("up", "references_panel.select_prev"),
            ("down", "references_panel.select_next"),
            ("enter", "references_panel.open"),
            ("escape", "peek.close"),
        ] {
            let ran = run(&keymap, os, keys, &list);
            assert_eq!(ran.as_deref(), Some(command), "{os:?} {keys}");
        }
    }
}
