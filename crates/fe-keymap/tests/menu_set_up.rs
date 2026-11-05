//! The Tenmoku menu's Set Up Tenmoku… item.

#[test]
fn set_up_item_follows_check_for_updates() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let tenmoku = bar.menu("Tenmoku").unwrap();
    let item = tenmoku.item("app.set_up").unwrap();
    assert_eq!(item.command().unwrap().title, "Set Up Tenmoku…");

    let ids: Vec<_> = (tenmoku.items.iter())
        .map(|item| item.id().unwrap_or("-"))
        .collect();
    let at = |id: &str| ids.iter().position(|item| *item == id).unwrap();
    assert_eq!(at("app.set_up"), at("app.check_for_updates") + 1, "{ids:?}");

    // The app menu is macOS's: elsewhere, the palette runs it.
    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    let linux = linux.unwrap();
    assert!(
        !linux
            .menus
            .iter()
            .any(|menu| menu.item("app.set_up").is_some())
    );
}
