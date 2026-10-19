//! The keys that add carets in vim's normal mode: `gb` everywhere, VS Code's
//! cmd keys on macOS and `alt-shift-up` and `down` elsewhere, only with
//! nothing pending; with vim off, VS Code's multi-cursor keys as before.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

const NEXT: &str = "editor.add_caret_at_next_occurrence";
const ALL: &str = "editor.add_carets_at_all_occurrences";
const ABOVE: &str = "editor.add_cursor_above";
const BELOW: &str = "editor.add_cursor_below";

/// The keys adding carets in vim's normal mode on `os`.
fn vim_keys(os: Os) -> Vec<(&'static str, &'static str)> {
    let mut keys = vec![("g b", NEXT)];
    if os == Os::MacOs {
        keys.extend([
            ("cmd-d", NEXT),
            ("cmd-shift-l", ALL),
            ("cmd-alt-up", ABOVE),
            ("cmd-alt-down", BELOW),
        ]);
    } else {
        keys.extend([("alt-shift-up", ABOVE), ("alt-shift-down", BELOW)]);
    }
    keys
}

/// The multi-cursor keys with vim off on `os`.
fn vim_off_keys(os: Os) -> Vec<(&'static str, &'static str)> {
    if os == Os::MacOs {
        vec![
            ("cmd-d", "editor.select_next_occurrence"),
            ("cmd-shift-l", "editor.select_all_occurrences"),
            ("cmd-alt-up", ABOVE),
            ("cmd-alt-down", BELOW),
            ("alt-shift-i", "editor.split_selection_into_lines"),
            ("escape", "editor.single_selection"),
        ]
    } else {
        vec![
            ("ctrl-d", "editor.select_next_occurrence"),
            ("ctrl-shift-l", "editor.select_all_occurrences"),
            ("alt-shift-up", ABOVE),
            ("alt-shift-down", BELOW),
            ("alt-shift-i", "editor.split_selection_into_lines"),
            ("escape", "editor.single_selection"),
        ]
    }
}

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(vim: Option<&str>, pending: Option<&str>) -> ContextStack {
    let mut editor = ContextFrame::new("editor");
    if let Some(mode) = vim {
        editor = editor.flag("vim.enabled").set("vim.mode", mode);
        if mode.starts_with("visual") {
            editor = editor.flag("vim.visual");
        }
    }
    if let Some(kind) = pending {
        editor = editor.flag("vim.pending").set("vim.pending_kind", kind);
    }
    [ContextFrame::new("workbench"), editor]
        .into_iter()
        .collect()
}

/// What typing `keys` runs in `stack`, if anything.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Pending;
    for key in keys.split(' ') {
        outcome = matcher.push(Keystroke::parse(key).unwrap(), stack);
        if outcome != KeyOutcome::Pending {
            break;
        }
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

fn each_keymap(check: impl Fn(&Keymap, Os)) {
    for (sources, os) in [
        ([DEFAULT_MACOS, DEFAULT], Os::MacOs),
        ([DEFAULT_LINUX, DEFAULT], Os::Linux),
    ] {
        for source in sources {
            check(&keymap(source, os), os);
        }
    }
}

#[test]
fn vim_multi_cursor_keys_resolve_in_normal_mode() {
    each_keymap(|keymap, os| {
        let normal = stack(Some("normal"), None);
        for (keys, command) in vim_keys(os) {
            assert_eq!(
                run(keymap, keys, &normal).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
            let elsewhere = [
                (Some("insert"), None),
                (Some("replace"), None),
                (Some("visual"), None),
                (Some("visual_line"), None),
                (Some("normal"), Some("operator")),
                (Some("normal"), Some("count")),
                (Some("normal"), Some("register")),
            ];
            for (mode, pending) in elsewhere {
                let got = run(keymap, keys, &stack(mode, pending));
                assert_ne!(
                    got.as_deref(),
                    Some(command),
                    "{os:?} {mode:?} {pending:?} {keys}"
                );
            }
        }
        // Off macOS ctrl-d and ctrl-shift-l stay vim's.
        if os != Os::MacOs {
            for keys in ["ctrl-d", "ctrl-shift-l"] {
                assert_eq!(run(keymap, keys, &normal), None, "{os:?} {keys}");
            }
        }
        // `g` then another key still reaches vim.
        for keys in ["g g", "g u", "g v"] {
            assert_eq!(run(keymap, keys, &normal), None, "{os:?} {keys}");
        }

        // With vim off, VS Code's multi-cursor keys as before, and `gb`
        // types.
        let off = stack(None, None);
        for (keys, command) in vim_off_keys(os) {
            assert_eq!(
                run(keymap, keys, &off).as_deref(),
                Some(command),
                "{os:?} vim off {keys}"
            );
        }
        assert_eq!(run(keymap, "g b", &off), None, "{os:?} vim off g b");
    });
}
