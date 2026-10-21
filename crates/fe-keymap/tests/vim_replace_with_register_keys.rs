//! With `vim.replace_with_register` in the editor's context, the `gr` keys
//! are vim's: T900's language-server `g r` rows match nothing, and the rest
//! of T900's keys keep their bindings.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(vim: Option<&str>, pending: Option<&str>) -> ContextStack {
    let mut editor = ContextFrame::new("editor");
    if let Some(mode) = vim {
        editor = editor
            .flag("vim.enabled")
            .flag("vim.replace_with_register")
            .set("vim.mode", mode);
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
fn replace_with_register_gives_gr_to_vim() {
    each_keymap(|keymap, os| {
        for keys in ["g r r", "g r n", "g r a", "g r i", "g r t"] {
            assert_eq!(
                run(keymap, keys, &stack(Some("normal"), None)),
                None,
                "{os:?} {keys}"
            );
        }
        assert_eq!(
            run(keymap, "g r a", &stack(Some("visual"), None)),
            None,
            "{os:?} visual g r a"
        );
        for (keys, command) in [
            ("g d", "nav.goto_definition"),
            ("g shift-d", "nav.goto_declaration"),
            ("ctrl-]", "nav.goto_definition"),
            ("g shift-o", "outline.show"),
            ("shift-k", "editor.show_hover"),
        ] {
            assert_eq!(
                run(keymap, keys, &stack(Some("normal"), None)).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
    });
}
