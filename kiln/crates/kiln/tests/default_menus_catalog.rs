// SPDX-License-Identifier: Apache-2.0

//! The bundled menus when the catalog drops the App menu: View and Window
//! keep their names, whatever the app is called.

use kiln::{
    Config,
    command::{CommandId, Value},
    core::{CatalogError, CommandCatalog, ItemKind, Os},
};

/// Knows every command but `app.*`.
struct NoAppCommands;

impl CommandCatalog for NoAppCommands {
    fn check(&self, command: &CommandId, _args: &Value) -> Result<(), CatalogError> {
        if command.as_str().starts_with("app.") {
            Err(CatalogError::UnknownCommand)
        } else {
            Ok(())
        }
    }
}

#[test]
fn a_dropped_app_menu_renames_no_other_menu() {
    for os in [Os::MacOs, Os::Linux, Os::Windows] {
        for name in ["Notes", "Window", "View"] {
            let (bar, _) = Config::new("com.example.X", name)
                .default_menus(true)
                .menu_bar_for(os, Some(&NoAppCommands));
            let menus: Vec<(&str, Vec<&str>)> = bar
                .menus
                .iter()
                .map(|menu| {
                    let commands = menu
                        .items
                        .iter()
                        .filter_map(|item| match &item.kind {
                            ItemKind::Command(command) => Some(command.command.as_str()),
                            _ => None,
                        })
                        .collect();
                    (menu.name.as_str(), commands)
                })
                .collect();
            assert_eq!(
                menus,
                vec![
                    (
                        "View",
                        vec!["command_palette.toggle", "window.toggle_fullscreen"]
                    ),
                    (
                        "Window",
                        vec!["window.minimize", "window.zoom", "window.close"]
                    ),
                ],
                "{name} on {os:?}"
            );
        }
    }
}
