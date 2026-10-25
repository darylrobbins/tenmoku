//! Peek Definition's keys: `alt-f12` peeks in the editor, as VS Code binds
//! it; in the peek, `escape` closes it and `enter` opens its file, over the
//! keys of the excerpt's editor and vim's; escape closes it from the
//! editor showing it too. The peek is drawn by the pane,
//! not inside the editor it's in, so its frame is the excerpt's parent.

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
fn alt_f12_peeks_in_the_editor() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for vim in [None, Some("normal")] {
            let peeked = run(&keymap, os, "alt-f12", &stack(&[editor(vim)]));
            assert_eq!(
                peeked.as_deref(),
                Some("editor.peek_definition"),
                "{os:?} {vim:?}"
            );
        }
        // Not outside an editor.
        let panel = stack(&[ContextFrame::new("project_panel")]);
        assert_eq!(run(&keymap, os, "alt-f12", &panel), None, "{os:?}");
    }
}

#[test]
fn escape_and_enter_in_the_peek() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        let peek = ContextFrame::new("peek");
        for vim in [None, Some("normal"), Some("visual")] {
            let excerpt = stack(&[peek.clone(), editor(vim)]);
            let closed = run(&keymap, os, "escape", &excerpt);
            assert_eq!(closed.as_deref(), Some("peek.close"), "{os:?} {vim:?}");
            let opened = run(&keymap, os, "enter", &excerpt);
            assert_eq!(opened.as_deref(), Some("peek.open"), "{os:?} {vim:?}");
        }
        // Before its excerpt is there.
        let closed = run(&keymap, os, "escape", &stack(std::slice::from_ref(&peek)));
        assert_eq!(closed.as_deref(), Some("peek.close"), "{os:?}");
        // The editor's own outside it.
        let outside = run(&keymap, os, "escape", &stack(&[editor(None)]));
        assert_eq!(
            outside.as_deref(),
            Some("editor.single_selection"),
            "{os:?}"
        );
        // In the editor showing it, but for its completion menu's and vim's
        // insert session's.
        let shown = |vim| editor(vim).flag("peek_shown");
        for vim in [None, Some("normal"), Some("visual")] {
            let closed = run(&keymap, os, "escape", &stack(&[shown(vim)]));
            assert_eq!(closed.as_deref(), Some("peek.close"), "{os:?} {vim:?}");
        }
        let menu = stack(&[shown(None).flag("completion")]);
        let dismissed = run(&keymap, os, "escape", &menu);
        assert_eq!(dismissed.as_deref(), Some("completion.dismiss"), "{os:?}");
        let insert = run(&keymap, os, "escape", &stack(&[shown(Some("insert"))]));
        assert_ne!(insert.as_deref(), Some("peek.close"), "{os:?}");
    }
}
