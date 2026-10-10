//! fastedit's one keymap and one menu table resolve, on each platform, to
//! exactly the per-platform files they replaced, which tests still use for
//! a platform's bindings on any host.

use std::path::Path;

use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_LINUX_MENUS, DEFAULT_MACOS,
    DEFAULT_MACOS_MENUS, KeymapLayer, MENUS, MenuBar, menu::DEFAULT_MENUS_PATH,
};
use kiln_core::Os;

/// Each binding of `source` as on `os`, without where it was read.
fn bindings(source: &str, os: Os) -> Vec<String> {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    layer
        .unwrap()
        .entries
        .iter()
        .map(|entry| {
            format!(
                "{} | {} | {} | {}",
                entry.keys,
                entry
                    .command
                    .as_ref()
                    .map_or("", |command| command.as_str()),
                entry
                    .context
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
                entry.args
            )
        })
        .collect()
}

#[test]
fn one_keymap_resolves_to_each_platforms() {
    assert_eq!(
        bindings(DEFAULT, Os::MacOs),
        bindings(DEFAULT_MACOS, Os::MacOs)
    );
    assert_eq!(
        bindings(DEFAULT, Os::Linux),
        bindings(DEFAULT_LINUX, Os::Linux)
    );
    // Windows has Linux's keys until it has its own.
    assert_eq!(
        bindings(DEFAULT, Os::Windows),
        bindings(DEFAULT_LINUX, Os::Windows)
    );
}

fn menus(source: &str, os: Os) -> MenuBar {
    let (bar, diagnostics) = MenuBar::parse_for(Path::new(DEFAULT_MENUS_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    bar.unwrap()
}

#[test]
fn one_menu_table_resolves_to_each_platforms() {
    assert_eq!(
        menus(MENUS, Os::MacOs),
        menus(DEFAULT_MACOS_MENUS, Os::MacOs)
    );
    assert_eq!(
        menus(MENUS, Os::Linux),
        menus(DEFAULT_LINUX_MENUS, Os::Linux)
    );
    assert_eq!(
        menus(MENUS, Os::Windows),
        menus(DEFAULT_LINUX_MENUS, Os::Windows)
    );
}
