//! Vim's `<C-w>` window keys run the pane commands in vim's normal mode, on
//! each platform, and nowhere else.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

const TABLE: &[(&str, &str)] = &[
    ("h", "pane.focus_left"),
    ("ctrl-h", "pane.focus_left"),
    ("left", "pane.focus_left"),
    ("j", "pane.focus_down"),
    ("ctrl-j", "pane.focus_down"),
    ("down", "pane.focus_down"),
    ("k", "pane.focus_up"),
    ("ctrl-k", "pane.focus_up"),
    ("up", "pane.focus_up"),
    ("l", "pane.focus_right"),
    ("ctrl-l", "pane.focus_right"),
    ("right", "pane.focus_right"),
    ("w", "pane.focus_next"),
    ("ctrl-w", "pane.focus_next"),
    ("shift-w", "pane.focus_previous"),
    ("s", "pane.split_down"),
    ("shift-s", "pane.split_down"),
    ("ctrl-s", "pane.split_down"),
    ("v", "pane.split_right"),
    ("ctrl-v", "pane.split_right"),
    ("c", "pane.close"),
    ("q", "pane.close"),
    ("ctrl-q", "pane.close"),
    ("shift-h", "pane.move_tab_left"),
    ("shift-j", "pane.move_tab_down"),
    ("shift-k", "pane.move_tab_up"),
    ("shift-l", "pane.move_tab_right"),
    (">", "pane.grow_width"),
    ("<", "pane.shrink_width"),
    ("+", "pane.grow_height"),
    ("-", "pane.shrink_height"),
    ("=", "pane.equalize"),
];

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
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

/// What pressing `ctrl-w` then `second` runs in `stack`, if anything.
fn run(keymap: &Keymap, second: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let first = matcher.push(Keystroke::parse("ctrl-w").unwrap(), stack);
    let outcome = match first {
        KeyOutcome::Pending => matcher.push(Keystroke::parse(second).unwrap(), stack),
        other => other,
    };
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

fn check(source: &str, os: Os) {
    let keymap = keymap(source, os);
    for &(keys, command) in TABLE {
        assert_eq!(
            run(&keymap, keys, &stack(Some("normal"))).as_deref(),
            Some(command),
            "{os:?} ctrl-w {keys}"
        );
        for other in [Some("insert"), Some("replace"), None] {
            let got = run(&keymap, keys, &stack(other));
            assert!(
                !got.as_deref().is_some_and(|c| c.starts_with("pane.")),
                "{os:?} {other:?} ctrl-w {keys} ran {got:?}"
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

/// Off macOS `ctrl-w` closes the tab, except as vim normal mode's prefix
/// and in vim's insert and replace mode, where it's vim's.
#[test]
fn ctrl_w_still_closes_the_tab_with_vim_off() {
    let keymap = keymap(DEFAULT, Os::Linux);
    for (stack, command) in [
        (stack(None), "workbench.close_tab"),
        (stack(Some("insert")), "vim.key"),
        (stack(Some("replace")), "vim.key"),
    ] {
        let mut matcher = Matcher::new(keymap.clone());
        let KeyOutcome::Dispatch(invocation) =
            matcher.push(Keystroke::parse("ctrl-w").unwrap(), &stack)
        else {
            panic!("ctrl-w waits");
        };
        assert_eq!(invocation.command.as_str(), command);
    }
    let mut matcher = Matcher::new(keymap);
    assert_eq!(
        matcher.push(Keystroke::parse("ctrl-w").unwrap(), &stack(Some("normal"))),
        KeyOutcome::Pending
    );
}
