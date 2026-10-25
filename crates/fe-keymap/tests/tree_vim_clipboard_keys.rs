//! Cut, copy and paste in the project panel: nvim-tree's `x`, `c` and `p`
//! with vim on, and VS Code's explorer's cmd/ctrl-x, -c and -v with it on
//! or off. None takes a key from an editor, and the name field's paste key
//! stays its own.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

/// The keys with vim on: (keys, command).
const VIM: &[(&str, &str)] = &[
    ("x", "project_panel.cut"),
    ("c", "project_panel.copy"),
    ("p", "project_panel.paste"),
];

/// The keys with vim on or off, by platform: (macOS keys, others', command).
const PLATFORM: &[(&str, &str, &str)] = &[
    ("cmd-x", "ctrl-x", "project_panel.cut"),
    ("cmd-c", "ctrl-c", "project_panel.copy"),
    ("cmd-v", "ctrl-v", "project_panel.paste"),
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

/// The panel's name field (the panel drops its `vim` flag while it's open).
fn name_field() -> ContextStack {
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

/// Asserts no key of `keys` runs a panel command in an editor.
fn not_in_editors(keymap: &Keymap, os: Os, keys: &str) {
    for mode in [Some("normal"), Some("visual"), Some("insert"), None] {
        let got = run(keymap, keys, &editor(mode));
        assert!(
            !got.as_deref()
                .is_some_and(|c| c.starts_with("project_panel.")),
            "{os:?} {keys} in an editor ({mode:?}) ran {got:?}"
        );
    }
}

fn check(source: &str, os: Os) {
    let keymap = keymap(source, os);
    for &(keys, command) in VIM {
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
        not_in_editors(&keymap, os, keys);
    }
    for &(mac, other, command) in PLATFORM {
        let keys = if os == Os::MacOs { mac } else { other };
        for vim in [false, true] {
            assert_eq!(
                run(&keymap, keys, &tree(vim)).as_deref(),
                Some(command),
                "{os:?} {keys} with vim {vim}"
            );
        }
        not_in_editors(&keymap, os, keys);
    }
    // The editor keeps its own clipboard keys.
    let paste = if os == Os::MacOs { "cmd-v" } else { "ctrl-v" };
    assert_eq!(
        run(&keymap, paste, &editor(None)).as_deref(),
        Some("editor.paste"),
        "{os:?}"
    );
    // The name field's paste is the field's.
    assert_eq!(
        run(&keymap, paste, &name_field()).as_deref(),
        Some("text_input.paste"),
        "{os:?}"
    );
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
