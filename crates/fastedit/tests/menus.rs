//! The built-in menu bar against every command the standalone app has.

#[test]
fn default_menus_know_every_command() {
    let (bar, diagnostics) =
        fe_gpui::fe_keymap::default_menus_macos(Some(&*fe_workbench::command_catalog()));
    assert_eq!(diagnostics, []);
    assert!(bar.is_some());
}
