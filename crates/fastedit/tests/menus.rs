//! The built-in menu bar against every command the standalone app has.

use fe_gpui::fe_keymap::MenuBar;

#[test]
fn default_menus_know_every_command() {
    let (bar, diagnostics) = MenuBar::default_macos(Some(&*fe_workbench::command_catalog()));
    assert_eq!(diagnostics, []);
    assert!(bar.is_some());
}
