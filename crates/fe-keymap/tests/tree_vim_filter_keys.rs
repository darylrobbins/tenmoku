//! Filtering the project tree by name with vim on: neo-tree's `/` opens
//! the filter and nvim-tree's `F` (shift-f) clears it. Neither runs with
//! vim off, nor in the filter's box (the panel drops its `vim` flag while
//! the box takes typing), and neither takes a key from an editor.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

/// The keys with vim on: (keys, command).
const TABLE: &[(&str, &str)] = &[
    ("/", "project_panel.filter"),
    ("shift-f", "project_panel.clear_filter"),
];

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench, then `frames`.
fn stack(frames: impl IntoIterator<Item = ContextFrame>) -> ContextStack {
    std::iter::once(ContextFrame::new("workbench"))
        .chain(frames)
        .collect()
}

/// The project panel, with the `vim` flag if `vim`.
fn tree(vim: bool) -> ContextStack {
    let frame = ContextFrame::new("project_panel");
    stack([if vim { frame.flag("vim") } else { frame }])
}

/// The filter's box, or the name field: the panel without its `vim` flag,
/// then the field's context.
fn text_input() -> ContextStack {
    stack([
        ContextFrame::new("project_panel"),
        ContextFrame::new("text_input"),
    ])
}

/// An editor with vim on in `mode`, or off.
fn editor(mode: Option<&str>) -> ContextStack {
    let frame = ContextFrame::new("editor");
    stack([match mode {
        Some(mode) => frame.flag("vim.enabled").set("vim.mode", mode),
        None => frame,
    }])
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
        assert_eq!(
            run(&keymap, keys, &text_input()),
            None,
            "{os:?} {keys} in the filter's box"
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
