//! Vim's window keys leave the Git log, as they do the other bottom tool
//! windows: with vim on, `<C-w>h/j/k/l`, their ctrl- forms, `<C-w>p` and
//! `<C-w>w` run the `dock.focus_*` commands and `dock.focus_editor`. Not
//! with vim off, nor in its filter box; bare `<C-h>/<C-j>/<C-k>/<C-l>` and
//! escape stay the list's.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

const WINDOW_KEYS: &[(&str, &str)] = &[
    ("ctrl-w h", "dock.focus_left"),
    ("ctrl-w ctrl-h", "dock.focus_left"),
    ("ctrl-w j", "dock.focus_down"),
    ("ctrl-w ctrl-j", "dock.focus_down"),
    ("ctrl-w k", "dock.focus_up"),
    ("ctrl-w ctrl-k", "dock.focus_up"),
    ("ctrl-w l", "dock.focus_right"),
    ("ctrl-w ctrl-l", "dock.focus_right"),
    ("ctrl-w p", "dock.focus_editor"),
    ("ctrl-w w", "dock.focus_editor"),
];

const BARE_KEYS: &[&str] = &["ctrl-h", "ctrl-j", "ctrl-k", "ctrl-l", "escape"];

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The Git log's panel, with the `vim` flag if `vim`, then `inner`.
fn git_log(vim: bool, inner: &[ContextFrame]) -> ContextStack {
    let panel = ContextFrame::new("git_log_panel");
    let panel = if vim { panel.flag("vim") } else { panel };
    std::iter::once(ContextFrame::new("workbench"))
        .chain([panel])
        .chain(inner.iter().cloned())
        .collect()
}

/// The command `keys` run in `stack`, a chord left pending running what the
/// chord timer would.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let keys = KeySequence::parse(keys).unwrap();
    let (last, first) = keys.keystrokes().split_last().unwrap();
    for keystroke in first {
        if matcher.push(*keystroke, stack) != KeyOutcome::Pending {
            return None;
        }
    }
    let outcome = match matcher.push(*last, stack) {
        KeyOutcome::Pending => matcher.flush(),
        outcome => outcome,
    };
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

fn is_dock(command: &Option<String>) -> bool {
    command
        .as_deref()
        .is_some_and(|got| got.starts_with("dock.focus_"))
}

fn check(source: &str, os: Os) {
    let keymap = keymap(source, os);
    for &(keys, command) in WINDOW_KEYS {
        assert_eq!(
            run(&keymap, keys, &git_log(true, &[])).as_deref(),
            Some(command),
            "{os:?} {keys}"
        );
        let off = run(&keymap, keys, &git_log(false, &[]));
        assert!(!is_dock(&off), "{os:?} {keys} with vim off ran {off:?}");
        let filter = [ContextFrame::new("text_input")];
        let got = run(&keymap, keys, &git_log(true, &filter));
        assert!(
            !is_dock(&got),
            "{os:?} {keys} in the filter box ran {got:?}"
        );
    }
    for keys in BARE_KEYS {
        let got = run(&keymap, keys, &git_log(true, &[]));
        assert!(!is_dock(&got), "{os:?} {keys} ran {got:?}");
    }
}

#[test]
fn the_window_keys_leave_the_git_log() {
    check(DEFAULT_MACOS, Os::MacOs);
    check(DEFAULT, Os::MacOs);
    check(DEFAULT_LINUX, Os::Linux);
    check(DEFAULT, Os::Linux);
}
