//! The Outline panel's list keys: up, down and Enter with vim on or off,
//! and with vim on `k`, `j`, `gg`, `G` and `o`, as the other lists have
//! them. None runs with vim off but the first three, and none takes a key
//! from an editor.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

/// The keys with vim on or off: (keys, command).
const ALWAYS: &[(&str, &str)] = &[
    ("up", "outline.select_previous"),
    ("down", "outline.select_next"),
    ("enter", "outline.open_selected"),
];

/// The keys with vim on: (keys, command).
const VIM: &[(&str, &str)] = &[
    ("k", "outline.select_previous"),
    ("j", "outline.select_next"),
    ("g g", "outline.select_first"),
    ("shift-g", "outline.select_last"),
    ("o", "outline.open_selected"),
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

/// The Outline panel, with the `vim` flag if `vim`.
fn outline(vim: bool) -> ContextStack {
    let frame = ContextFrame::new("outline_panel");
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
    for &(keys, command) in ALWAYS {
        for vim in [false, true] {
            assert_eq!(
                run(&keymap, keys, &outline(vim)).as_deref(),
                Some(command),
                "{os:?} {keys} vim {vim}"
            );
        }
    }
    for &(keys, command) in VIM {
        assert_eq!(
            run(&keymap, keys, &outline(true)).as_deref(),
            Some(command),
            "{os:?} {keys}"
        );
    }
    for keys in ["k", "j", "o"] {
        assert_eq!(
            run(&keymap, keys, &outline(false)),
            None,
            "{os:?} {keys} with vim off"
        );
    }
    for &(keys, _) in ALWAYS.iter().chain(VIM) {
        for mode in [Some("normal"), Some("visual"), Some("insert"), None] {
            let got = run(&keymap, keys, &editor(mode));
            assert!(
                !got.as_deref().is_some_and(|c| c.starts_with("outline.")),
                "{os:?} {keys} in an editor ({mode:?}) ran {got:?}"
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
