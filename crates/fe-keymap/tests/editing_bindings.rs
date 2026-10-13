//! The row, comment and go-to-line commands' keys, on each platform, and
//! that none of them takes a key from vim.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, Keymap, KeymapLayer, Keystroke};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[ContextFrame]) -> ContextStack {
    frames.iter().cloned().collect()
}

fn editor() -> ContextStack {
    stack(&[ContextFrame::new("workbench"), ContextFrame::new("editor")])
}

fn find_bar() -> ContextStack {
    stack(&[
        ContextFrame::new("workbench"),
        ContextFrame::new("find_bar"),
        ContextFrame::new("text_input"),
    ])
}

/// The editor with vim on in `mode`, as the key context reports it.
fn vim(mode: &str) -> ContextStack {
    let visual = mode.starts_with("visual");
    let mut frame = ContextFrame::new("editor")
        .flag("vim.enabled")
        .set("vim.mode", mode);
    if visual {
        frame = frame.flag("vim.visual");
    }
    stack(&[ContextFrame::new("workbench"), frame])
}

fn resolves(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let keystroke = Keystroke::parse(keys).unwrap();
    keymap
        .lookup(&keystroke, stack)
        .map(|invocation| invocation.command.as_str().to_owned())
}

fn check(os: Os, table: &[(&str, &str)], go_to_line: &str) {
    let keymap = keymap(os);
    for &(keys, command) in table {
        assert_eq!(
            resolves(&keymap, keys, &editor()).as_deref(),
            Some(command),
            "{os:?} {keys}"
        );
    }
    assert_eq!(
        resolves(&keymap, go_to_line, &editor()).as_deref(),
        Some("go_to_line.toggle"),
        "{os:?}"
    );
    assert_eq!(
        resolves(&keymap, "ctrl-g", &editor()).as_deref(),
        Some("go_to_line.toggle"),
        "{os:?} ctrl-g"
    );
}

#[test]
fn macos() {
    check(
        Os::MacOs,
        &[
            ("cmd-]", "editor.indent"),
            ("cmd-[", "editor.outdent"),
            ("shift-tab", "editor.outdent"),
            ("alt-up", "editor.move_rows_up"),
            ("alt-down", "editor.move_rows_down"),
            ("shift-alt-down", "editor.duplicate_rows"),
            ("cmd-shift-k", "editor.delete_rows"),
            ("cmd-/", "editor.toggle_comment"),
        ],
        "ctrl-g",
    );
    let keymap = keymap(Os::MacOs);
    for context in [editor(), find_bar()] {
        assert_eq!(
            resolves(&keymap, "cmd-g", &context).as_deref(),
            Some("find.next")
        );
        assert_eq!(
            resolves(&keymap, "cmd-shift-g", &context).as_deref(),
            Some("find.previous")
        );
    }
}

#[test]
fn linux() {
    check(
        Os::Linux,
        &[
            ("ctrl-]", "editor.indent"),
            ("ctrl-[", "editor.outdent"),
            ("shift-tab", "editor.outdent"),
            ("alt-up", "editor.move_rows_up"),
            ("alt-down", "editor.move_rows_down"),
            ("ctrl-shift-alt-down", "editor.duplicate_rows"),
            ("ctrl-shift-k", "editor.delete_rows"),
            ("ctrl-/", "editor.toggle_comment"),
        ],
        "ctrl-g",
    );
    let keymap = keymap(Os::Linux);
    for context in [editor(), find_bar()] {
        assert_eq!(
            resolves(&keymap, "f3", &context).as_deref(),
            Some("find.next")
        );
        assert_eq!(
            resolves(&keymap, "shift-f3", &context).as_deref(),
            Some("find.previous")
        );
    }
    assert_eq!(
        resolves(&keymap, "enter", &find_bar()).as_deref(),
        Some("find.next")
    );
    assert_eq!(
        resolves(&keymap, "shift-enter", &find_bar()).as_deref(),
        Some("find.previous")
    );
}

/// With vim on, every key a vim binding has in a mode still reaches vim's.
#[test]
fn vim_keeps_its_keys() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for mode in [
            "normal",
            "insert",
            "replace",
            "visual",
            "visual_line",
            "visual_block",
        ] {
            let stack = vim(mode);
            let vim_keys: Vec<_> = keymap
                .bindings()
                .iter()
                .filter(|binding| binding.command.as_str().starts_with("vim."))
                .filter(|binding| binding.context.as_ref().is_none_or(|c| c.eval(&stack)))
                .filter(|binding| binding.keys.keystrokes().len() == 1)
                .map(|binding| binding.keys.keystrokes()[0])
                .collect();
            assert!(!vim_keys.is_empty(), "{os:?} {mode}");
            for keystroke in vim_keys {
                let command = keymap
                    .lookup(&keystroke, &stack)
                    .map(|invocation| invocation.command.as_str().to_owned());
                assert!(
                    command.as_deref().is_some_and(|c| c.starts_with("vim.")),
                    "{os:?} {mode} {keystroke}: {command:?}"
                );
            }
        }
    }
    let linux = keymap(Os::Linux);
    assert_eq!(
        resolves(&linux, "ctrl-[", &vim("insert")).as_deref(),
        Some("vim.normal_mode")
    );
}

/// Keys vim's key handler reads (shift-tab as tab, alt-up as up) stay vim's
/// outside insert and replace mode: no editor command takes them.
#[test]
fn vim_modes_keep_the_keys_its_handler_reads() {
    for (os, duplicate) in [
        (Os::MacOs, "shift-alt-down"),
        (Os::Linux, "ctrl-shift-alt-down"),
    ] {
        let keymap = keymap(os);
        for mode in [
            "normal",
            "operator_pending",
            "visual",
            "visual_line",
            "visual_block",
        ] {
            for keys in ["shift-tab", "alt-up", "alt-down", duplicate, "ctrl-["] {
                let command = resolves(&keymap, keys, &vim(mode));
                assert!(
                    command.as_deref().is_none_or(|c| c.starts_with("vim.")),
                    "{os:?} {mode} {keys}: {command:?}"
                );
            }
        }
        for mode in ["insert", "replace"] {
            assert_eq!(
                resolves(&keymap, "alt-up", &vim(mode)).as_deref(),
                Some("editor.move_rows_up"),
                "{os:?} {mode}"
            );
        }
    }
}
