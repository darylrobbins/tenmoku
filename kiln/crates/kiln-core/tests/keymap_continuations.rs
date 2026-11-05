// SPDX-License-Identifier: Apache-2.0

//! [`Keymap::continuations`]: the keys that go on from a chord held, for a
//! which-key popup.

use std::path::Path;

use kiln_command::{CommandId, ContextFrame, ContextStack};
use kiln_core::{Continuation, Keymap, KeymapLayer, Keystroke, Os};

const DEFAULTS: &str = r#"
[[bindings]]
keys = "g b"
command = "editor.add_caret"
context = "editor"

[[bindings]]
keys = "g d"
command = "nav.goto_definition"
context = "editor"

[[bindings]]
keys = "g r r"
command = "nav.find_references"
context = "editor"

[[bindings]]
keys = "g r n"
command = "lsp.rename"
context = "editor"

[[bindings]]
keys = "g x"
command = "panel.open"
context = "panel"

[[bindings]]
keys = "g"
command = "editor.go"
context = "editor"

[[bindings]]
keys = "ctrl-w h"
command = "pane.focus_left"
"#;

/// The user's layer: `g d` again, later, in the editor, and `g y` in the
/// editor, then in the workbench around it.
const USER: &str = r#"
[[bindings]]
keys = "g d"
command = "nav.goto_declaration"
context = "editor"

[[bindings]]
keys = "g y"
command = "editor.copy_path"
context = "editor"

[[bindings]]
keys = "g y"
command = "workbench.copy_path"
context = "workbench"
"#;

fn layer(name: &str, source: &str) -> KeymapLayer {
    let (layer, diagnostics) = KeymapLayer::parse_for(Path::new(name), source, None, Os::MacOs);
    assert_eq!(diagnostics, []);
    layer.expect("the keymap loads")
}

fn keys(keys: &str) -> Vec<Keystroke> {
    keys.split(' ')
        .map(|key| Keystroke::parse(key).unwrap())
        .collect()
}

fn continuation(next: &str, command: Option<&str>, longer: bool) -> Continuation {
    Continuation {
        next: Keystroke::parse(next).unwrap(),
        command: command.map(|id| CommandId::parse(id).unwrap()),
        longer,
    }
}

/// After `g` in an editor: each next key once, in the order of its first
/// binding, with what it runs, `r` only going on; `g x`, a panel's, is
/// absent, and `g` itself, as long as what's held, too. As in matching, of
/// the two `g d` the user's, later, wins, and of the two `g y` the
/// editor's, the deeper context match, though the workbench's comes later.
/// In a panel, `g x` and the workbench's `g y` go on. Nothing goes on from
/// a key no binding starts with, and `ctrl-w h`, with
/// no context, holds everywhere.
#[test]
fn keymap_continuations_group_by_next_key() {
    let keymap = Keymap::from_layers([layer("defaults.toml", DEFAULTS), layer("user.toml", USER)]);
    let editor = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    assert_eq!(
        keymap.continuations(&keys("g"), &editor),
        [
            continuation("b", Some("editor.add_caret"), false),
            continuation("d", Some("nav.goto_declaration"), false),
            continuation("r", None, true),
            continuation("y", Some("editor.copy_path"), false),
        ]
    );
    assert_eq!(
        keymap.continuations(&keys("g r"), &editor),
        [
            continuation("r", Some("nav.find_references"), false),
            continuation("n", Some("lsp.rename"), false),
        ]
    );

    let panel = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("panel"));
    assert_eq!(
        keymap.continuations(&keys("g"), &panel),
        [
            continuation("x", Some("panel.open"), false),
            continuation("y", Some("workbench.copy_path"), false),
        ]
    );
    assert_eq!(keymap.continuations(&keys("z"), &editor), []);
    assert_eq!(keymap.continuations(&keys("g r r"), &editor), []);
    assert_eq!(
        keymap.continuations(&keys("ctrl-w"), &panel),
        [continuation("h", Some("pane.focus_left"), false)]
    );
}

/// A key bound both alone after the held ones and as the start of longer
/// bindings has its command and goes on.
#[test]
fn a_key_both_bound_and_held_goes_on() {
    let source = r#"
[[bindings]]
keys = "cmd-k cmd-s"
command = "keymap.open"

[[bindings]]
keys = "cmd-k cmd-s cmd-d"
command = "keymap.defaults"
"#;
    let keymap = Keymap::from_layers([layer("keymap.toml", source)]);
    assert_eq!(
        keymap.continuations(&keys("cmd-k"), &ContextStack::new()),
        [continuation("cmd-s", Some("keymap.open"), true)]
    );
}
