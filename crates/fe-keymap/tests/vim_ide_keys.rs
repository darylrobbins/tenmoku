//! Vim's `gd`, `gD`, `ctrl-]`, nvim 0.12's `gr` keys, `gO` and `K` run the
//! language-server commands in vim's normal mode, on each platform, and
//! nowhere else.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

const TABLE: &[(&str, &str)] = &[
    ("g d", "nav.goto_definition"),
    ("ctrl-]", "nav.goto_definition"),
    ("g shift-d", "nav.goto_declaration"),
    ("g r r", "nav.find_references"),
    ("g r n", "lsp.rename"),
    ("g r a", "lsp.code_actions"),
    ("g r i", "nav.goto_implementation"),
    ("g r t", "nav.goto_type_definition"),
    ("g shift-o", "outline.show"),
    ("shift-k", "editor.show_hover"),
];

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
fn vim_ide_keys_resolve_in_normal_mode() {
    each_keymap(|keymap, os| {
        for &(keys, command) in TABLE {
            assert_eq!(
                run(keymap, keys, &stack(Some("normal"), None)).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
        assert_eq!(
            run(keymap, "g r a", &stack(Some("visual"), None)).as_deref(),
            Some("lsp.code_actions"),
            "{os:?} visual g r a"
        );
    });
}

#[test]
fn vim_ide_keys_leave_vims_keys() {
    each_keymap(|keymap, os| {
        for &(keys, command) in TABLE {
            let cases = [
                (Some("insert"), None),
                (Some("replace"), None),
                (Some("normal"), Some("operator")),
                (Some("normal"), Some("count")),
                (Some("normal"), Some("register")),
                (None, None),
            ];
            for (mode, pending) in cases {
                let got = run(keymap, keys, &stack(mode, pending));
                assert_ne!(
                    got.as_deref(),
                    Some(command),
                    "{os:?} {mode:?} {pending:?} {keys}"
                );
            }
            if keys != "g r a" {
                let got = run(keymap, keys, &stack(Some("visual"), None));
                assert_ne!(got.as_deref(), Some(command), "{os:?} visual {keys}");
            }
        }
        for keys in ["g g", "g r x", "g u"] {
            assert_eq!(
                run(keymap, keys, &stack(Some("normal"), None)),
                None,
                "{os:?} {keys}"
            );
        }
    });
}
