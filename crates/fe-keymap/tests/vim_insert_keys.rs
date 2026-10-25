//! nvim's insert-mode ctrl keys reach vim in vim's insert and replace mode,
//! on each platform, and resolve as they did before everywhere else.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

const KEYS: [&str; 9] = [
    "ctrl-w", "ctrl-u", "ctrl-r", "ctrl-t", "ctrl-d", "ctrl-o", "ctrl-n", "ctrl-p", "ctrl-a",
];

/// The context of the bindings that hand them to vim.
const CONTEXT: &str = r#"context = "editor && (vim.mode == insert || vim.mode == replace)""#;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// `source` without the bindings in [`CONTEXT`]: the keymap as it was
/// before they were added.
fn without_insert_keys(source: &str) -> String {
    let mut blocks = source.split("[[bindings]]");
    let mut out = blocks.next().unwrap_or_default().to_owned();
    for block in blocks {
        if !block.lines().any(|line| line.trim() == CONTEXT) {
            out.push_str("[[bindings]]");
            out.push_str(block);
        }
    }
    out
}

fn stack(vim: Option<&str>) -> ContextStack {
    let mut editor = ContextFrame::new("editor");
    if let Some(mode) = vim {
        editor = editor.flag("vim.enabled").set("vim.mode", mode);
    }
    [ContextFrame::new("workbench"), editor]
        .into_iter()
        .collect()
}

/// What pressing `key` does in `stack`: the command it runs, `pending` for
/// a chord's first key, or `unmatched`.
fn resolve(keymap: &Keymap, key: &str, stack: &ContextStack) -> String {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(key).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) | KeyOutcome::DispatchPending(invocation) => {
            invocation.command.as_str().to_owned()
        }
        KeyOutcome::Pending => "pending".to_owned(),
        KeyOutcome::Unmatched(_) => "unmatched".to_owned(),
    }
}

#[test]
fn insert_ctrl_keys_are_vims() {
    for (os, sources) in [
        (Os::Linux, [DEFAULT_LINUX, DEFAULT]),
        (Os::MacOs, [DEFAULT_MACOS, DEFAULT]),
    ] {
        for source in sources {
            let now = keymap(source, os);
            let before = keymap(&without_insert_keys(source), os);
            for key in KEYS {
                for mode in ["insert", "replace"] {
                    let got = resolve(&now, key, &stack(Some(mode)));
                    match os {
                        Os::MacOs => assert_eq!(got, "unmatched", "{os:?} {mode} {key}"),
                        _ => assert_eq!(got, "vim.key", "{os:?} {mode} {key}"),
                    }
                }
                for other in [Some("normal"), None] {
                    assert_eq!(
                        resolve(&now, key, &stack(other)),
                        resolve(&before, key, &stack(other)),
                        "{os:?} {other:?} {key}"
                    );
                }
            }
        }
    }
    // With vim off, off macOS, the workbench's keys still run.
    let linux = keymap(DEFAULT_LINUX, Os::Linux);
    for (key, command) in [
        ("ctrl-w", "workbench.close_tab"),
        ("ctrl-r", "workbench.open_recent"),
        ("ctrl-p", "quick_open.toggle"),
    ] {
        assert_eq!(resolve(&linux, key, &stack(None)), command, "{key}");
    }
    // In vim normal mode `ctrl-w` starts the window keys.
    assert_eq!(resolve(&linux, "ctrl-w", &stack(Some("normal"))), "pending");
}
