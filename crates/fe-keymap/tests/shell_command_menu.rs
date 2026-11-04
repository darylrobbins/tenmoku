//! The Install/Uninstall 'tenmoku' Command items.

fn ids(menu: &fe_keymap::Menu) -> Vec<&str> {
    menu.items.iter().map(|i| i.id().unwrap_or("-")).collect()
}

#[test]
fn macos_app_menu_has_the_install_item_after_keyboard_shortcuts_json_and_linux_file_menu_beside_settings()
 {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let ids = ids(bar.menu("Tenmoku").unwrap());
    let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
    let json = at("workbench.open_keyboard_shortcuts_json");
    assert_eq!(at("app.install_shell_command"), json + 1, "{ids:?}");
    assert_eq!(at("app.uninstall_shell_command"), json + 2, "{ids:?}");

    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    let linux = linux.unwrap();
    let ids = ids_of(&linux, "File");
    let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
    let json = at("workbench.open_keyboard_shortcuts_json");
    assert_eq!(at("app.install_shell_command"), json + 1, "{ids:?}");
    assert_eq!(at("app.uninstall_shell_command"), json + 2, "{ids:?}");
    assert!(at("workbench.open_settings_ui") < json);
}

fn ids_of<'a>(bar: &'a fe_keymap::MenuBar, name: &str) -> Vec<&'a str> {
    ids(bar.menu(name).unwrap())
}
