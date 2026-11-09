// SPDX-License-Identifier: Apache-2.0

//! kiln-core's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists; then, in en-US, the
//! text it shows and writes byte for byte as it was before it moved into
//! the catalogue: kiln's bundled menus (whose keys are kiln-core's), a
//! Services item's default title, the shortcut key names and the trust
//! file's header.

use std::path::Path;

use kiln_core::{
    ItemKind, KeySequence, MenuBar, Os, macos_label, menu::set_label_arg, pc_label,
    trust::TrustStore,
};
#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &kiln_core::l10n::RESOURCES).check();
}

/// kiln's bundled menus, as on `os`: each menu's title, then its items'
/// titles, as parsed and as their labels resolve.
fn bundled_menus(os: Os) -> Vec<(String, Vec<String>)> {
    let source = include_str!("../../kiln/src/setup/menus.toml");
    let (bar, diagnostics) = MenuBar::parse_for(Path::new("menus.toml"), source, None, os);
    assert_eq!(diagnostics, [], "on {os:?}");
    let bar = bar.unwrap();
    (bar.menus.iter())
        .map(|menu| {
            assert_eq!(menu.label().resolve(), menu.name, "on {os:?}");
            let items = (menu.items.iter())
                .filter_map(|item| match &item.kind {
                    ItemKind::Command(command) => {
                        assert!(command.title_key.is_some(), "{command:?}");
                        assert_eq!(command.label().resolve(), command.title, "on {os:?}");
                        Some(command.title.clone())
                    }
                    _ => None,
                })
                .collect();
            (menu.name.clone(), items)
        })
        .collect()
}

/// The titles kiln's `menus.toml` wrote literally, `{app}` put in, before
/// they were keys.
#[test]
fn en_us_menus_are_unchanged() {
    set_label_arg("app", "Notes");
    let owned = |names: &[&str]| {
        names
            .iter()
            .map(|&name| name.to_owned())
            .collect::<Vec<_>>()
    };
    let view = (
        "View".to_owned(),
        owned(&["Command Palette", "Toggle Full Screen"]),
    );
    let window = (
        "Window".to_owned(),
        owned(&["Minimize", "Zoom", "Close Window"]),
    );
    let mac_app = (
        "App".to_owned(),
        owned(&["Hide Notes", "Hide Others", "Show All", "Quit Notes"]),
    );
    assert_eq!(
        bundled_menus(Os::MacOs),
        [mac_app, view.clone(), window.clone()]
    );
    let app = ("App".to_owned(), owned(&["Quit Notes"]));
    for os in [Os::Linux, Os::Windows] {
        assert_eq!(
            bundled_menus(os),
            [app.clone(), view.clone(), window.clone()]
        );
    }

    // A Services item with no title of its own.
    let source = "[[menus]]\nname = \"Notes\"\n\n[[menus.items]]\nsystem = \"services\"\n";
    let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), source, None);
    assert_eq!(diagnostics, []);
    let ItemKind::System(services) = &bar.unwrap().menus[0].items[0].kind else {
        panic!("a system item")
    };
    assert_eq!(services.title, "Services");
    assert_eq!(services.label().key(), Some("kiln-core-menu-services"));
    assert_eq!(services.label().resolve(), "Services");
}

#[test]
fn en_us_key_names_are_unchanged() {
    let pc = |keys: &str| pc_label(&KeySequence::parse_for(keys, Os::Linux).unwrap());
    assert_eq!(pc("ctrl-alt-shift-cmd-left"), "Ctrl+Alt+Shift+Super+Left");
    let named = [
        ("right", "Right"),
        ("up", "Up"),
        ("down", "Down"),
        ("enter", "Enter"),
        ("escape", "Esc"),
        ("backspace", "Backspace"),
        ("delete", "Delete"),
        ("tab", "Tab"),
        ("space", "Space"),
        ("ctrl-click", "Ctrl+Click"),
        ("pageup", "PageUp"),
        ("pagedown", "PageDown"),
        ("home", "Home"),
        ("end", "End"),
        ("shift-f5", "Shift+F5"),
        ("ctrl-k ctrl-a", "Ctrl+K Ctrl+A"),
    ];
    for (keys, label) in named {
        assert_eq!(pc(keys), label, "{keys}");
    }
    let mac = |keys: &str| macos_label(&KeySequence::parse_for(keys, Os::MacOs).unwrap());
    assert_eq!(mac("cmd-space"), "⌘Space");
    assert_eq!(mac("cmd-click"), "⌘Click");
    assert_eq!(mac("cmd-shift-left"), "⇧⌘←");
}

#[test]
fn en_us_trust_file_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trust.toml");
    TrustStore::load(&path)
        .set(Path::new("/projects/notes"), true)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "\
# This file records the folders you trusted (true) or didn't (false) to
# run their code. Subfolders follow the nearest folder listed. The app's
# Trust This Folder and Untrust This Folder commands change one.

[folders]
\"/projects/notes\" = true
"
    );
}
