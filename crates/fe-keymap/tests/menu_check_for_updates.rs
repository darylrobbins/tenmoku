//! The Tenmoku menu's Check for Updates… item.

#[test]
fn tenmoku_menu_has_item() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let tenmoku = bar.menu("Tenmoku").unwrap();
    let item = tenmoku.item("app.check_for_updates").unwrap();
    assert_eq!(item.command().unwrap().title, "Check for Updates…");

    // Past Hide and Show All, before Settings….
    let ids: Vec<_> = (tenmoku.items.iter())
        .map(|item| item.id().unwrap_or("-"))
        .collect();
    let at = |id: &str| ids.iter().position(|item| *item == id).unwrap();
    assert!(at("app.show_all") < at("app.check_for_updates"), "{ids:?}");
    assert!(
        at("app.check_for_updates") < at("workbench.open_settings_ui"),
        "{ids:?}"
    );

    // The app menu is macOS's: elsewhere, the palette runs it.
    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    let linux = linux.unwrap();
    let in_a_menu = |menu: &fe_keymap::Menu| menu.item("app.check_for_updates").is_some();
    assert!(!linux.menus.iter().any(in_a_menu));
}
