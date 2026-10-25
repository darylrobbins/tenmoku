//! The unimpaired pairs `]b` `[b` (tabs) and `]q` `[q` (the Problems list in
//! quickfix's place): only in an editor in vim's normal mode with nothing
//! pending.

use std::collections::HashSet;
use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

const KEYS: [(&str, &str); 4] = [
    ("] b", "workbench.next_tab"),
    ("[ b", "workbench.previous_tab"),
    ("] q", "diagnostics.next"),
    ("[ q", "diagnostics.previous"),
];

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(vim: Option<&str>, pending: Option<&str>, editor: bool) -> ContextStack {
    let mut frame = ContextFrame::new(if editor { "editor" } else { "project_panel" });
    if let Some(mode) = vim {
        frame = frame.flag("vim.enabled").set("vim.mode", mode);
        if mode.starts_with("visual") {
            frame = frame.flag("vim.visual");
        }
    }
    if let Some(kind) = pending {
        frame = frame.flag("vim.pending").set("vim.pending_kind", kind);
    }
    [ContextFrame::new("workbench"), frame]
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
fn vim_bracket_pairs_resolve_in_normal_mode() {
    each_keymap(|keymap, os| {
        let normal = stack(Some("normal"), None, true);
        for (keys, command) in KEYS {
            assert_eq!(
                run(keymap, keys, &normal).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
        // `]d` and `[d` are unchanged.
        assert_eq!(
            run(keymap, "] d", &normal).as_deref(),
            Some("diagnostics.next")
        );
        assert_eq!(
            run(keymap, "[ d", &normal).as_deref(),
            Some("diagnostics.previous")
        );
    });
}

#[test]
fn vim_bracket_pairs_stay_out_of_other_modes() {
    each_keymap(|keymap, os| {
        let elsewhere = [
            stack(None, None, true),
            stack(Some("insert"), None, true),
            stack(Some("visual"), None, true),
            stack(Some("normal"), Some("operator"), true),
            stack(Some("normal"), None, false),
        ];
        for (keys, _) in KEYS {
            for stack in &elsewhere {
                assert_eq!(run(keymap, keys, stack), None, "{os:?} {keys}");
            }
        }
    });
}

#[test]
fn default_keymaps_have_no_duplicate_bindings() {
    each_keymap(|keymap, os| {
        let mut seen = HashSet::new();
        for binding in keymap.bindings() {
            let context = binding.context.as_ref().map(ToString::to_string);
            assert!(
                seen.insert((binding.keys.to_string(), context.clone())),
                "{os:?} {} is bound twice in {context:?}",
                binding.keys
            );
        }
    });
}
