//! The Help menu: last, after Window, on macOS and Linux, with What's New.

use std::path::Path;

use fe_command::Value;
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer, MenuBar, shortcuts,
};

fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers(vec![layer.expect("the default keymap loads")])
}

fn check(bar: MenuBar, keymap: &Keymap, os: &str) {
    let names: Vec<_> = bar.menus.iter().map(|menu| menu.name.as_str()).collect();
    assert_eq!(names[names.len() - 2..], ["Window", "Help"], "{os}");
    let help = bar.menus.last().unwrap();
    assert_eq!(help.title_key.as_deref(), Some("keymap-menu-help"), "{os}");
    let items: Vec<_> = (help.items.iter())
        .map(|item| item.command().expect("a command item"))
        .collect();
    assert_eq!(items.len(), 1, "{os}");
    let whats_new = items[0];
    assert_eq!(whats_new.title, "What's New", "{os}");
    assert_eq!(
        whats_new.title_key.as_deref(),
        Some("keymap-menu-whats-new"),
        "{os}"
    );
    assert_eq!(whats_new.command.as_str(), "app.show_whats_new", "{os}");
    assert_eq!(whats_new.args, Value::Null, "{os}");
    assert!(whats_new.alternatives.is_empty(), "{os}");
    // No default key: the item shows no shortcut.
    assert_eq!(whats_new.shortcut(keymap), None, "{os}");
    assert_eq!(
        shortcuts(
            keymap,
            &whats_new.command,
            &Value::Null,
            &whats_new.shortcut_context
        ),
        [],
        "{os}"
    );
}

#[test]
fn help_is_last_with_whats_new_on_macos() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    check(bar.unwrap(), &keymap(DEFAULT_MACOS), "macOS");
}

#[test]
fn help_is_last_with_whats_new_on_linux() {
    let (bar, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    check(bar.unwrap(), &keymap(DEFAULT_LINUX), "Linux");
}
