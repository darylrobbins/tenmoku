//! The References, Problems and Search results lists' vim keys: with vim
//! on, `k` and `j` in References, and `gg`, `G` and `o` in all three, as
//! the tree has them. None runs with vim off, and none takes a key from an
//! editor.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

/// Each list's context and its new keys with vim on: (keys, command).
const LISTS: &[(&str, &[(&str, &str)])] = &[
    (
        "ReferencesPanel",
        &[
            ("k", "references_panel.select_prev"),
            ("j", "references_panel.select_next"),
            ("g g", "references_panel.select_first"),
            ("shift-g", "references_panel.select_last"),
            ("o", "references_panel.open"),
        ],
    ),
    (
        "problems_panel",
        &[
            ("g g", "problems.select_first"),
            ("shift-g", "problems.select_last"),
            ("o", "problems.open"),
        ],
    ),
    (
        "search_results",
        &[
            ("g g", "search.select_first"),
            ("shift-g", "search.select_last"),
            ("o", "search.open_selected"),
        ],
    ),
];

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench, then `frame`.
fn stack(frame: ContextFrame) -> ContextStack {
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

/// The list of `context`, with the `vim` flag if `vim`.
fn list(context: &str, vim: bool) -> ContextStack {
    let frame = ContextFrame::new(context);
    stack(if vim { frame.flag("vim") } else { frame })
}

/// An editor with vim on in `mode`, or off.
fn editor(mode: Option<&str>) -> ContextStack {
    let frame = ContextFrame::new("editor");
    stack(match mode {
        Some(mode) => frame.flag("vim.enabled").set("vim.mode", mode),
        None => frame,
    })
}

/// The command `keys` run in `stack`, pressed one after another, if any.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let keys = KeySequence::parse(keys).unwrap();
    let (last, first) = keys.keystrokes().split_last().unwrap();
    for keystroke in first {
        if matcher.push(*keystroke, stack) != KeyOutcome::Pending {
            return None;
        }
    }
    match matcher.push(*last, stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

fn check(source: &str, os: Os) {
    let keymap = keymap(source, os);
    let commands: Vec<&str> = (LISTS.iter())
        .flat_map(|(_, keys)| keys.iter().map(|&(_, command)| command))
        .collect();
    for &(context, keys) in LISTS {
        for &(keys, command) in keys {
            assert_eq!(
                run(&keymap, keys, &list(context, true)).as_deref(),
                Some(command),
                "{os:?} {context} {keys}"
            );
            assert_eq!(
                run(&keymap, keys, &list(context, false)),
                None,
                "{os:?} {context} {keys} with vim off"
            );
            for mode in [Some("normal"), Some("visual"), Some("insert"), None] {
                let got = run(&keymap, keys, &editor(mode));
                assert!(
                    !got.as_deref().is_some_and(|got| commands.contains(&got)),
                    "{os:?} {keys} in an editor ({mode:?}) ran {got:?}"
                );
            }
        }
    }
    // No binding of the lists' commands is in an editor's context.
    for binding in keymap.bindings() {
        if commands.contains(&binding.command.as_str()) {
            let context = binding.context.as_ref().map(ToString::to_string);
            assert!(
                !context.as_deref().unwrap_or("").contains("editor"),
                "{os:?} {} bound in {context:?}",
                binding.command.as_str()
            );
        }
    }
}

#[test]
fn macos() {
    check(DEFAULT_MACOS, Os::MacOs);
    check(DEFAULT, Os::MacOs);
}

#[test]
fn linux() {
    check(DEFAULT_LINUX, Os::Linux);
    check(DEFAULT, Os::Linux);
}
