//! nvim-tree's tree keys with vim on, in the project panel: `o` toggles a
//! folder (or opens a file), `E` and `W` expand and collapse every folder,
//! `H` toggles ignored entries and `g r` refreshes. None runs with vim off,
//! where `enter` still opens, and none takes a key from an editor.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

/// The keys with vim on: (keys, command).
const TABLE: &[(&str, &str)] = &[
    ("o", "project_panel.toggle"),
    ("shift-e", "project_panel.expand_all"),
    ("shift-w", "project_panel.collapse_all"),
    ("shift-h", "project_panel.toggle_ignored"),
    ("g r", "project_panel.refresh"),
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

/// The project panel, with the `vim` flag if `vim`.
fn tree(vim: bool) -> ContextStack {
    let frame = ContextFrame::new("project_panel");
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
    for &(keys, command) in TABLE {
        assert_eq!(
            run(&keymap, keys, &tree(true)).as_deref(),
            Some(command),
            "{os:?} {keys}"
        );
        assert_eq!(
            run(&keymap, keys, &tree(false)),
            None,
            "{os:?} {keys} with vim off"
        );
        for mode in [Some("normal"), Some("visual"), Some("insert"), None] {
            let got = run(&keymap, keys, &editor(mode));
            assert!(
                !got.as_deref()
                    .is_some_and(|c| c.starts_with("project_panel.")),
                "{os:?} {keys} in an editor ({mode:?}) ran {got:?}"
            );
        }
    }
    // `enter` opens with vim on or off: a folder toggles, as `o`'s does.
    for vim in [true, false] {
        assert_eq!(
            run(&keymap, "enter", &tree(vim)).as_deref(),
            Some("project_panel.open"),
            "{os:?} vim {vim}"
        );
    }
    // The keys they share a first key with keep theirs.
    for (keys, command) in [
        ("g g", "project_panel.select_first"),
        ("g y", "project_panel.copy_path"),
        ("shift-r", "project_panel.rename"),
        ("h", "project_panel.collapse_or_parent"),
        ("l", "project_panel.open"),
    ] {
        assert_eq!(
            run(&keymap, keys, &tree(true)).as_deref(),
            Some(command),
            "{os:?} {keys}"
        );
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
