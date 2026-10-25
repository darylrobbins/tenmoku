//! Vim's window keys back out of a dock: with vim on, in the sidebar's
//! lists `<C-w>h/j/k/l` (and with ctrl on the second key) run
//! `dock.focus_left` and the rest, `<C-w>p` and `<C-w>w`
//! `dock.focus_editor`, as do bare `<C-h>/<C-j>/<C-k>/<C-l>` and escape; in
//! the terminal only the `<C-w>` ones. None runs with vim off, in an
//! editor (the Changes panel's commit box among them) or in an input.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Matcher,
};
use kiln_core::Os;

/// The `<C-w>` keys, in the lists and the terminal: (keys, command).
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

/// The keys only the lists have: (keys, command).
const LIST_KEYS: &[(&str, &str)] = &[
    ("ctrl-h", "dock.focus_left"),
    ("ctrl-j", "dock.focus_down"),
    ("ctrl-k", "dock.focus_up"),
    ("ctrl-l", "dock.focus_right"),
    ("escape", "dock.focus_editor"),
];

/// The frames around each list, the last one setting the `vim` flag:
/// search's results sit in its panel.
const LISTS: &[&[&str]] = &[
    &["project_panel"],
    &["outline_panel"],
    &["ReferencesPanel"],
    &["problems_panel"],
    &["search_panel", "search_results"],
    &["git_panel"],
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

/// The frames named `names`, the last with the `vim` flag if `vim`.
fn frames(names: &[&str], vim: bool) -> Vec<ContextFrame> {
    let (last, outer) = names.split_last().unwrap();
    let last = ContextFrame::new(*last);
    (outer.iter().map(|name| ContextFrame::new(*name)))
        .chain([if vim { last.flag("vim") } else { last }])
        .collect()
}

/// An editor with vim on in `mode`, or off.
fn editor(mode: Option<&str>) -> ContextFrame {
    let frame = ContextFrame::new("editor");
    match mode {
        Some(mode) => frame.flag("vim.enabled").set("vim.mode", mode),
        None => frame,
    }
}

/// The terminal, in its panel with the `vim` flag if `vim`.
fn terminal(vim: bool) -> ContextStack {
    let mut frames = frames(&["terminal_panel"], vim);
    frames.push(ContextFrame::new("terminal"));
    stack(frames)
}

/// The command `keys` run in `stack`, pressed one after another, if any;
/// a chord left pending runs what the chord timer would.
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
    let all = || WINDOW_KEYS.iter().chain(LIST_KEYS);
    for list in LISTS {
        for &(keys, command) in all() {
            assert_eq!(
                run(&keymap, keys, &stack(frames(list, true))).as_deref(),
                Some(command),
                "{os:?} {list:?} {keys}"
            );
            let off = run(&keymap, keys, &stack(frames(list, false)));
            assert!(
                !is_dock(&off),
                "{os:?} {list:?} {keys} with vim off ran {off:?}"
            );
        }
    }
    for &(keys, command) in WINDOW_KEYS {
        assert_eq!(
            run(&keymap, keys, &terminal(true)).as_deref(),
            Some(command),
            "{os:?} terminal {keys}"
        );
        let off = run(&keymap, keys, &terminal(false));
        assert!(
            !is_dock(&off),
            "{os:?} terminal {keys} with vim off ran {off:?}"
        );
    }
    for &(keys, _) in LIST_KEYS {
        let got = run(&keymap, keys, &terminal(true));
        assert!(!is_dock(&got), "{os:?} terminal {keys} ran {got:?}");
    }

    // Not in an editor, alone or in the Changes panel (its commit box), nor
    // in an input: search's, the tree's name field, the terminal's find row.
    let mut elsewhere = Vec::new();
    for mode in [Some("normal"), Some("visual"), Some("insert"), None] {
        elsewhere.push(stack([editor(mode)]));
        let mut commit_box = frames(&["git_panel"], true);
        commit_box.push(editor(mode));
        elsewhere.push(stack(commit_box));
    }
    let input = || ContextFrame::new("text_input");
    elsewhere.push(stack([ContextFrame::new("search_panel"), input()]));
    for vim in [true, false] {
        let mut field = frames(&["project_panel"], vim);
        field.push(input());
        elsewhere.push(stack(field));
        let mut find = frames(&["terminal_panel"], vim);
        find.extend([ContextFrame::new("find_bar"), input()]);
        elsewhere.push(stack(find));
    }
    for stack in &elsewhere {
        for &(keys, _) in all() {
            let got = run(&keymap, keys, stack);
            assert!(!is_dock(&got), "{os:?} {keys} in {stack:?} ran {got:?}");
        }
    }
    // Each binding of the commands rules out an editor and an input.
    for binding in keymap.bindings() {
        if binding.command.as_str().starts_with("dock.focus_") {
            let context = binding.context.as_ref().map(ToString::to_string);
            let context = context.as_deref().unwrap_or("");
            assert!(
                context.ends_with("&& vim && !editor && !text_input"),
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
