// SPDX-License-Identifier: LicenseRef-Proprietary

//! A native menu item shows its newest shortcut that macOS can display.

use std::path::Path;

use gpui::TestAppContext;
use kiln_core::{Keymap, KeymapLayer, MenuBar, Os};
use kiln_platform::{
    commands, keys,
    menus::{self, KEY_CONTEXT, MenuCommand},
};

mod pad {
    kiln_platform::commands!(
        pad,
        [
            /// Clear
            Clear,
        ]
    );
}

const MENUS: &str = r#"
[[menus]]
name = "Pad"

[[menus.items]]
title = "Services"
system = "services"

[[menus.items]]
separator = true

[[menus.items]]
title = "Quit"
command = "app.quit"

[[menus]]
name = "Edit"

[[menus.items]]
title = "Select All"
command = "pad.select_all"
os_action = "select_all"

[[menus.items]]
title = "Fold All"
command = "pad.fold_all"

[[menus.items]]
title = "Clear"
command = "pad.clear"

[[menus.items]]
title = "Save"
command = "pad.save"

[[menus]]
name = "View"

[[menus.items]]
title = "Toggle Full Screen"
command = "window.toggle_fullscreen"

[[menus]]
name = "Window"

[[menus.items]]
title = "Close Window"
command = "window.close"

[[menus.items]]
title = "Minimize"
command = "window.minimize"

[[menus.items]]
title = "Zoom"
command = "window.zoom"
"#;

const KEYS: &str = r#"
[[bindings]]
keys = "secondary-a"
command = "pad.select_all"

[[bindings]]
keys = "secondary-k secondary-0"
command = "pad.fold_all"

[[bindings]]
keys = "secondary-s"
command = "pad.save"
"#;

fn bar() -> MenuBar {
    let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), MENUS, None);
    assert_eq!(diagnostics, []);
    bar.unwrap()
}

/// The keymap of `KEYS` under a user layer of `user`.
fn keymap(user: &str) -> Keymap {
    let layer = |source: &str| {
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
        assert_eq!(diagnostics, []);
        layer.unwrap()
    };
    Keymap::from_layers([layer(KEYS), layer(user)])
}

/// `keys` with this platform's `secondary`, in GPUI's spelling.
fn gpui_keys(keys: &str) -> String {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    gpui::Keystroke::parse(&keys.replace("secondary", secondary))
        .unwrap()
        .unparse()
}

/// The app's commands and keys, then the menus installed.
fn install(cx: &mut TestAppContext) {
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
        keys::init(cx, keymap(""));
        menus::install(cx, bar());
    });
}

fn command(menu: &str, item: &str) -> MenuCommand {
    MenuCommand {
        menu: menu.into(),
        item: item.into(),
    }
}

/// Each GPUI key binding for `action`, as `(keys, context)`.
fn bindings_for(cx: &mut TestAppContext, action: &MenuCommand) -> Vec<(String, String)> {
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings_for_action(action)
            .map(|binding| {
                let keys: Vec<_> = binding
                    .keystrokes()
                    .iter()
                    .map(|keystroke| keystroke.inner().unparse())
                    .collect();
                let context = binding.predicate().map(|p| p.to_string());
                (keys.join(" "), context.unwrap_or_default())
            })
            .collect()
    })
}

#[gpui::test]
fn an_older_native_shortcut_is_shown_when_the_newest_has_none(cx: &mut TestAppContext) {
    install(cx);
    let save = command("Edit", "pad.save");
    for keys in ["secondary-enter", "ctrl-tab", "secondary-fn-s"] {
        let user = format!(
            r#"
            [[bindings]]
            keys = "{keys}"
            command = "pad.save"
            "#
        );
        cx.update(|cx| keys::set_keymap(cx, keymap(&user)));
        assert_eq!(
            bindings_for(cx, &save),
            [(gpui_keys("secondary-s"), KEY_CONTEXT.to_owned())],
            "{keys}"
        );
    }
    let user = r#"
        [[bindings]]
        keys = "secondary-enter"
        command = "pad.save"

        [[bindings]]
        keys = "secondary-shift-s"
        command = "pad.save"
        "#;
    cx.update(|cx| keys::set_keymap(cx, keymap(user)));
    assert_eq!(
        bindings_for(cx, &save),
        [(gpui_keys("secondary-shift-s"), KEY_CONTEXT.to_owned())]
    );
}
