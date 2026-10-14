//! The View menu's Toggle Terminal item.

use std::path::Path;

use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, KeySequence, Keymap, KeymapLayer, MenuBar};

fn view_ids(bar: &MenuBar) -> Vec<String> {
    let view = bar.menu("View").unwrap();
    view.items
        .iter()
        .map(|item| item.id().unwrap_or("-").to_owned())
        .collect()
}

fn assert_after_dock(bar: MenuBar) {
    let ids = view_ids(&bar);
    let dock = ids.iter().position(|id| id == "dock.toggle").unwrap();
    assert_eq!(ids[dock + 1], "terminal.toggle", "{ids:?}");
    let item = bar
        .menu("View")
        .and_then(|menu| menu.item("terminal.toggle"))
        .and_then(|item| item.command())
        .unwrap();
    assert_eq!(item.title, "Toggle Terminal");
}

#[test]
fn toggle_terminal_follows_toggle_dock() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    assert_after_dock(bar.unwrap());
    let (bar, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    assert_after_dock(bar.unwrap());
}

#[test]
fn toggle_terminal_shortcut_is_ctrl_backtick() {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let keymap = Keymap::from_layers(vec![default.unwrap()]);
    let (bar, _) = fe_keymap::default_menus_macos(None);
    let bar = bar.unwrap();
    let item = bar
        .menu("View")
        .and_then(|menu| menu.item("terminal.toggle"))
        .and_then(|item| item.command())
        .unwrap();
    assert_eq!(
        item.shortcut(&keymap),
        Some(KeySequence::parse("ctrl-`").unwrap())
    );
}
