// SPDX-License-Identifier: LicenseRef-Proprietary

//! A chord prefix with a binding of its own in a deeper context runs at
//! once and keeps listening; a prefix with none is pending as ever.

use std::path::Path;

use kiln_command::{ContextFrame, ContextStack};
use kiln_core::{KeyOutcome, Keymap, KeymapLayer, Keystroke, Matcher};

const KEYS: &str = r#"
[[bindings]]
keys = "escape"
command = "editor.cancel"
context = "editor"

[[bindings]]
keys = "escape escape"
command = "workbench.leave"
context = "zen"

[[bindings]]
keys = "secondary-k secondary-0"
command = "editor.fold_all"
context = "editor"
"#;

fn matcher() -> Matcher {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), KEYS, None);
    assert_eq!(diagnostics, []);
    Matcher::new(Keymap::from_layers([layer.unwrap()]))
}

fn key(source: &str) -> Keystroke {
    Keystroke::parse(source).unwrap()
}

fn stack() -> ContextStack {
    ["workbench", "zen", "editor"]
        .into_iter()
        .map(ContextFrame::new)
        .collect()
}

fn ran(outcome: &KeyOutcome) -> &str {
    match outcome {
        KeyOutcome::Dispatch(run) | KeyOutcome::DispatchPending(run) => run.command.as_str(),
        other => panic!("ran nothing: {other:?}"),
    }
}

#[test]
fn prefix_with_own_binding_runs_now() {
    let stack = stack();
    let mut matcher = matcher();

    // One escape runs the deeper binding at once, and the chord carries on.
    let first = matcher.push(key("escape"), &stack);
    assert!(matches!(first, KeyOutcome::DispatchPending(_)), "{first:?}");
    assert_eq!(ran(&first), "editor.cancel");
    let second = matcher.push(key("escape"), &stack);
    assert!(matches!(second, KeyOutcome::Dispatch(_)), "{second:?}");
    assert_eq!(ran(&second), "workbench.leave");
    assert!(matcher.pending().is_empty());

    // A different second key is a fresh keystroke, and nothing is handed back.
    matcher.push(key("escape"), &stack);
    assert_eq!(
        matcher.push(key("x"), &stack),
        KeyOutcome::Unmatched(vec![key("x")])
    );
    matcher.push(key("escape"), &stack);
    let fresh = matcher.push(key("escape"), &stack);
    assert_eq!(ran(&fresh), "workbench.leave");

    // A timeout dispatches nothing more, and hands nothing back to replay.
    let first = matcher.push(key("escape"), &stack);
    assert_eq!(ran(&first), "editor.cancel");
    assert_eq!(matcher.flush(), KeyOutcome::Unmatched(Vec::new()));
    assert!(matcher.pending().is_empty());
}

#[test]
fn pure_prefix_unchanged() {
    let stack = stack();
    let mut matcher = matcher();
    assert_eq!(
        matcher.push(key("secondary-k"), &stack),
        KeyOutcome::Pending
    );
    let done = matcher.push(key("secondary-0"), &stack);
    assert_eq!(ran(&done), "editor.fold_all");

    matcher.push(key("secondary-k"), &stack);
    assert_eq!(
        matcher.push(key("x"), &stack),
        KeyOutcome::Unmatched(vec![key("secondary-k"), key("x")])
    );
    matcher.push(key("secondary-k"), &stack);
    assert_eq!(
        matcher.flush(),
        KeyOutcome::Unmatched(vec![key("secondary-k")])
    );
}

#[test]
fn a_context_free_chord_does_not_delay_a_bare_key() {
    let source = r#"
[[bindings]]
keys = "escape"
command = "editor.cancel"
context = "editor"

[[bindings]]
keys = "escape escape"
command = "app.leave"
"#;
    let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
    assert_eq!(diagnostics, []);
    let mut matcher = Matcher::new(Keymap::from_layers([layer.unwrap()]));
    let first = matcher.push(key("escape"), &stack());
    assert!(matches!(first, KeyOutcome::DispatchPending(_)), "{first:?}");
}
