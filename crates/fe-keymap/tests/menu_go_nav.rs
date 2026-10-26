//! The Go menu lists the navigation commands, with shortcuts from the default keymap.

use std::path::Path;

use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, KeySequence, Keymap, KeymapLayer, MenuBar};

fn default_bar() -> MenuBar {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    bar.expect("the default menus load")
}

fn default_keymap() -> Keymap {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    Keymap::from_layers(vec![default.expect("the default keymap loads")])
}

/// Item IDs, `-` for a separator.
fn ids(bar: &MenuBar, menu: &str) -> Vec<String> {
    bar.menu(menu)
        .unwrap()
        .items
        .iter()
        .map(|item| item.id().unwrap_or("-").to_owned())
        .collect()
}

#[test]
fn go_menu_lists_navigation_commands() {
    assert_eq!(
        ids(&default_bar(), "Go"),
        [
            "nav.back",
            "nav.forward",
            "-",
            "quick_open.toggle",
            "go_to_line.toggle",
            "editor.go_to_bracket",
            "nav.workspace_symbols",
            "-",
            "nav.goto_definition",
            "nav.goto_declaration",
            "nav.goto_type_definition",
            "nav.goto_implementation",
            "editor.peek_references",
            "nav.find_references",
            "-",
            "workbench.next_tab",
            "workbench.previous_tab",
            "-",
            "pane.focus_left",
            "pane.focus_right",
            "pane.focus_up",
            "pane.focus_down",
            "-",
            "project_panel.reveal_active",
            "-",
            "diagnostics.next",
            "diagnostics.previous",
            "git.next_change",
            "git.previous_change",
        ]
    );
}

#[test]
fn go_navigation_shortcuts_follow_the_default_keymap() {
    let bar = default_bar();
    let keymap = default_keymap();
    for (id, keys) in [
        ("nav.back", "ctrl--"),
        ("nav.forward", "ctrl-shift--"),
        ("nav.workspace_symbols", "cmd-t"),
        ("nav.goto_definition", "f12"),
        ("editor.peek_references", "shift-f12"),
        ("nav.find_references", "shift-alt-f12"),
    ] {
        let item = bar
            .menu("Go")
            .and_then(|m| m.item(id))
            .and_then(|i| i.command());
        let shortcut = item
            .unwrap_or_else(|| panic!("Go > {id}"))
            .shortcut(&keymap);
        assert_eq!(shortcut, Some(KeySequence::parse(keys).unwrap()), "{id}");
    }
}
