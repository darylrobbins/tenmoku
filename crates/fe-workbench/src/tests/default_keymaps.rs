//! The built-in keymaps against every command, and the Linux keymap
//! against the macOS one.

use std::{collections::BTreeSet, path::Path};

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer, KeymapStore, Keystroke,
};

use crate::command_catalog;

fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers([layer.expect("the default keymap loads")])
}

fn commands(keymap: &Keymap) -> BTreeSet<String> {
    keymap
        .bindings()
        .iter()
        .map(|binding| binding.command.as_str().to_owned())
        .collect()
}

/// What `keys` runs in an editor in the workbench whose frame has `vim`'s
/// keys, if any.
fn run(keymap: &Keymap, keys: &str, vim: Option<(&str, bool)>) -> Option<String> {
    let mut editor = ContextFrame::new("editor");
    if let Some((mode, visual)) = vim {
        editor = editor.set("vim.mode", mode);
        if visual {
            editor = editor.flag("vim.visual");
        }
    }
    let stack: ContextStack = [ContextFrame::new("workbench"), editor]
        .into_iter()
        .collect();
    let keystroke: Keystroke = keys.parse().unwrap();
    keymap
        .lookup(&keystroke, &stack)
        .map(|invocation| invocation.command.as_str().to_owned())
}

#[test]
fn every_default_binding_names_a_known_command() {
    for source in [DEFAULT_MACOS, DEFAULT_LINUX] {
        let store = KeymapStore::load(source, None, Some(command_catalog()));
        assert_eq!(store.diagnostics(), []);
    }
}

#[test]
fn linux_keymap_binds_the_macos_commands_with_ctrl() {
    let macos = keymap(DEFAULT_MACOS);
    let linux = keymap(DEFAULT_LINUX);
    // macOS's own, with no Linux convention to follow.
    let macos_only = ["app.hide", "app.hide_others", "window.minimize"];
    let mut expected: BTreeSet<_> = commands(&macos)
        .into_iter()
        .filter(|command| !macos_only.contains(&command.as_str()))
        .collect();
    // Linux's own: its menu bar is in the window.
    expected.insert("menu_bar.open".to_owned());
    assert_eq!(commands(&linux), expected);
    for binding in linux.bindings() {
        let cmd = binding.keys.keystrokes().iter().any(|k| k.modifiers.cmd);
        assert!(!cmd, "{} uses cmd", binding.command.as_str());
    }
    for (keys, command) in [
        ("ctrl-z", "editor.undo"),
        ("ctrl-y", "editor.redo"),
        ("ctrl-shift-z", "editor.redo"),
        ("ctrl-c", "editor.copy"),
        ("ctrl-v", "editor.paste"),
        ("ctrl-s", "editor.save"),
        ("ctrl-left", "editor.move_word_left"),
        ("ctrl-home", "editor.move_document_start"),
        ("ctrl-p", "quick_open.toggle"),
        ("ctrl-w", "workbench.close_tab"),
        ("ctrl-q", "app.quit"),
        // Shifted punctuation, by what it types (see default-linux.toml).
        ("ctrl-{", "editor.fold"),
        ("ctrl-}", "editor.unfold"),
        ("ctrl-_", "nav.forward"),
    ] {
        assert_eq!(run(&linux, keys, None).as_deref(), Some(command), "{keys}");
    }
}

#[test]
fn vim_keeps_its_ctrl_keys_on_linux() {
    let linux = keymap(DEFAULT_LINUX);
    for keys in ["ctrl-v", "ctrl-q", "ctrl-y", "ctrl-f", "ctrl-b"] {
        for vim in [("normal", false), ("visual", true)] {
            assert_eq!(
                run(&linux, keys, Some(vim)).as_deref(),
                Some("vim.key"),
                "{keys} in {vim:?}"
            );
        }
        assert_ne!(
            run(&linux, keys, Some(("insert", false))).as_deref(),
            Some("vim.key")
        );
    }
    assert_eq!(
        run(&linux, "ctrl-y", Some(("insert", false))).as_deref(),
        Some("editor.redo")
    );
    assert_eq!(run(&linux, "ctrl-r", Some(("normal", false))), None);
}

#[test]
fn the_linux_menus_name_known_commands() {
    let (bar, diagnostics) = fe_keymap::default_menus_linux(Some(&*command_catalog()));
    assert_eq!(diagnostics, []);
    assert!(bar.is_some());
}
