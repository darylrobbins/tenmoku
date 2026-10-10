// SPDX-License-Identifier: LicenseRef-Proprietary

//! A click as a bindable key: `cmd-click` parses and prints, binds only as
//! one modified keystroke, and [`crate::Keymap::lookup`] finds what it runs.

use std::path::Path;

use crate::Severity;

use super::{keymap, keystroke, stack};
use crate::{Key, KeyOutcome, KeymapLayer, Keystroke, Matcher, Modifiers};

#[test]
fn click_parses_only_with_modifiers() {
    // Parses, with its modifiers, and prints back the same.
    let cmd = Keystroke::parse("cmd-click").unwrap();
    let cmd_mods = Modifiers {
        cmd: true,
        ..Modifiers::default()
    };
    assert_eq!(cmd, Keystroke::new(cmd_mods, Key::Click));
    assert_eq!(cmd.to_string(), "cmd-click");
    let alt_shift = Keystroke::parse("alt-shift-click").unwrap();
    let alt_shift_mods = Modifiers {
        alt: true,
        shift: true,
        ..Modifiers::default()
    };
    assert_eq!(alt_shift, Keystroke::new(alt_shift_mods, Key::Click));
    assert_eq!(alt_shift.to_string(), "alt-shift-click");

    // A click alone, in a chord or with another key is one warning per
    // binding, and only that binding is skipped.
    let source = "\
[[bindings]]
keys = \"click\"
command = \"editor.save\"

[[bindings]]
keys = \"cmd-k click\"
command = \"editor.save\"

[[bindings]]
keys = \"click a\"
command = \"editor.save\"

[[bindings]]
keys = \"cmd-click\"
command = \"editor.fold_all\"
context = \"editor\"
";
    let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
    let found: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.severity, diagnostic.key.clone(), diagnostic.line))
        .collect();
    let expected = [(0, 2), (1, 6), (2, 10)].map(|(ix, line)| {
        let key = Some(format!("bindings[{ix}].keys"));
        (Severity::Warning, key, line)
    });
    assert_eq!(found, expected);
    assert!(diagnostics[1].message.contains("`cmd-k click`"));
    let layer = layer.expect("warnings don't reject the file");
    let kept: Vec<_> = layer
        .entries
        .iter()
        .map(|entry| entry.keys.to_string())
        .collect();
    assert_eq!(kept, ["cmd-click"]);

    // Lookup: the matcher's precedence, the deepest context first, then the
    // later layer or entry; `None` where nothing applies.
    let defaults = "\
[[bindings]]
keys = \"cmd-click\"
command = \"nav.goto_definition\"
context = \"workbench\"

[[bindings]]
keys = \"cmd-click\"
command = \"editor.peek\"
context = \"editor\"

[[bindings]]
keys = \"cmd-click\"
command = \"editor.later\"
context = \"workbench\"
";
    let user = "\
[[bindings]]
keys = \"alt-click\"
command = \"editor.add_cursor\"
context = \"editor\"
";
    let keymap = keymap(defaults, user);
    let lookup = |keys: &str, frames: &[&str]| {
        keymap
            .lookup(&keystroke(keys), &stack(frames))
            .map(|invocation| (invocation.command.to_string(), invocation.context))
    };
    let workbench = stack(&["workbench"]);
    let both = stack(&["workbench", "editor"]);
    assert_eq!(
        lookup("cmd-click", &["workbench", "editor"]),
        Some(("editor.peek".into(), both.clone()))
    );
    assert_eq!(
        lookup("cmd-click", &["workbench"]),
        Some(("editor.later".into(), workbench))
    );
    assert_eq!(
        lookup("alt-click", &["workbench", "editor"]).map(|(command, _)| command),
        Some("editor.add_cursor".into())
    );
    assert_eq!(lookup("cmd-click", &["terminal"]), None);
    assert_eq!(lookup("ctrl-click", &["workbench", "editor"]), None);
    assert_eq!(lookup("alt-click", &["workbench"]), None);

    // The same as the matcher would dispatch for that one keystroke.
    let mut matcher = Matcher::new(keymap.clone());
    let KeyOutcome::Dispatch(matched) = matcher.push(keystroke("cmd-click"), &both) else {
        panic!("cmd-click is bound");
    };
    assert_eq!(keymap.lookup(&keystroke("cmd-click"), &both), Some(matched));
}
