// SPDX-License-Identifier: LicenseRef-Proprietary

//! A menu item's shortcut is a key, never a mouse click.

use std::path::Path;

use kiln_core::{ItemKind, Keymap, KeymapLayer, MenuBar, Os};

const MENUS: &str = r#"
[[menus]]
name = "Go"

[[menus.items]]
title = "Go to Definition"
command = "nav.goto_definition"
"#;

/// The Go item's shortcut under bindings of `keys`, in order, as written.
fn shortcut(keys: &[&str]) -> Option<String> {
    let source: String = keys
        .iter()
        .map(|keys| {
            format!("[[bindings]]\nkeys = \"{keys}\"\ncommand = \"nav.goto_definition\"\n\n")
        })
        .collect();
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new("keymap.toml"), &source, None, Os::MacOs);
    assert_eq!(diagnostics, []);
    let keymap = Keymap::from_layers(vec![layer.expect("the keymap loads")]);
    let (bar, diagnostics) = MenuBar::parse_for(Path::new("menus.toml"), MENUS, None, Os::MacOs);
    assert_eq!(diagnostics, []);
    let bar = bar.expect("the menus load");
    let item = &bar.menu("Go").unwrap().items[0];
    let ItemKind::Command(item) = &item.kind else {
        panic!("the Go item is a command");
    };
    item.shortcut(&keymap).map(|keys| keys.to_string())
}

#[test]
fn a_later_click_binding_does_not_become_the_menu_shortcut() {
    assert_eq!(shortcut(&["f12", "cmd-click"]).as_deref(), Some("f12"));
}

#[test]
fn a_command_bound_only_to_a_click_has_no_menu_shortcut() {
    assert_eq!(shortcut(&["cmd-click"]), None);
}

#[test]
fn the_newest_key_binding_is_still_the_menu_shortcut() {
    assert_eq!(
        shortcut(&["cmd-click", "f12", "cmd-b"]).as_deref(),
        Some("cmd-b")
    );
}
