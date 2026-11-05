// SPDX-License-Identifier: Apache-2.0

//! One keymap and one menu table for every platform: `secondary`, keys by
//! platform, and `platform` filters, each checked as on every platform.

use std::path::Path;

use crate::{
    ItemKind, KeySequence, KeymapLayer, Keystroke, MenuBar, Os, Severity, platform::PlatformFilter,
};

fn layer(source: &str, os: Os) -> (Option<KeymapLayer>, Vec<crate::Diagnostic>) {
    KeymapLayer::parse_for(Path::new("keymap.toml"), source, None, os)
}

/// The bindings `source` gives on `os`, as `keys command` lines.
fn bindings(source: &str, os: Os) -> Vec<String> {
    let (layer, diagnostics) = layer(source, os);
    assert_eq!(diagnostics, [], "{os:?}");
    layer
        .unwrap()
        .entries
        .iter()
        .map(|entry| {
            let command = entry.command.as_ref().map_or("", |c| c.as_str());
            format!("{} {command}", entry.keys)
        })
        .collect()
}

fn errors(source: &str, os: Os) -> Vec<String> {
    let (_, diagnostics) = layer(source, os);
    diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

#[test]
fn secondary_is_cmd_on_macos_and_ctrl_elsewhere() {
    let keystroke = |source, os| Keystroke::parse_for(source, os).unwrap().to_string();
    assert_eq!(keystroke("secondary-s", Os::MacOs), "cmd-s");
    assert_eq!(keystroke("secondary-s", Os::Linux), "ctrl-s");
    assert_eq!(keystroke("secondary-s", Os::Windows), "ctrl-s");
    assert_eq!(keystroke("Secondary-Shift-p", Os::MacOs), "cmd-shift-p");
    let chord = KeySequence::parse_for("secondary-k secondary-0", Os::Linux).unwrap();
    assert_eq!(chord.to_string(), "ctrl-k ctrl-0");
    // On Linux secondary is ctrl, so naming both is naming ctrl twice.
    assert!(Keystroke::parse_for("secondary-ctrl-s", Os::Linux).is_err());
    assert_eq!(keystroke("secondary-ctrl-s", Os::MacOs), "cmd-ctrl-s");
}

#[test]
fn keys_can_differ_by_platform() {
    let source = r#"
        [[bindings]]
        keys = "secondary-s"
        command = "app.save"

        [[bindings]]
        keys = { macos = "alt-left", default = "ctrl-left" }
        command = "editor.word_left"

        [[bindings]]
        keys = { macos = "cmd-h" }
        command = "app.hide"

        [[bindings]]
        keys = { linux = "ctrl-{", windows = "ctrl-shift-[" }
        command = "editor.fold"
    "#;
    assert_eq!(
        bindings(source, Os::MacOs),
        [
            "cmd-s app.save",
            "alt-left editor.word_left",
            "cmd-h app.hide"
        ]
    );
    assert_eq!(
        bindings(source, Os::Linux),
        [
            "ctrl-s app.save",
            "ctrl-left editor.word_left",
            "ctrl-{ editor.fold"
        ]
    );
    assert_eq!(
        bindings(source, Os::Windows),
        [
            "ctrl-s app.save",
            "ctrl-left editor.word_left",
            "ctrl-shift-[ editor.fold"
        ]
    );
}

#[test]
fn platform_filters_leave_bindings_out() {
    let source = r#"
        [[bindings]]
        keys = "cmd-h"
        command = "app.hide"
        platform = "macos"

        [[bindings]]
        keys = "f10"
        command = "menu_bar.open"
        platform = "!macos"

        [[bindings]]
        keys = "ctrl-y"
        command = "app.redo"
        platform = "linux, windows"
    "#;
    assert_eq!(bindings(source, Os::MacOs), ["cmd-h app.hide"]);
    assert_eq!(
        bindings(source, Os::Linux),
        ["f10 menu_bar.open", "ctrl-y app.redo"]
    );
}

#[test]
fn every_platforms_keys_and_filter_are_checked() {
    // A mistake in Linux's keys shows on macOS too.
    let bad_linux = r#"
        [[bindings]]
        keys = { macos = "cmd-s", linux = "ctrl-nope-s" }
        command = "app.save"
    "#;
    for os in Os::ALL {
        let found = errors(bad_linux, os);
        assert!(
            found.iter().any(|e| e.contains("nope")),
            "{os:?}: {found:?}"
        );
    }
    let unknown_platform = r#"
        [[bindings]]
        keys = { amiga = "cmd-s" }
        command = "app.save"
    "#;
    assert!(errors(unknown_platform, Os::Linux)[0].contains("unknown platform \"amiga\""));
    let bad_filter = r#"
        [[bindings]]
        keys = "cmd-s"
        command = "app.save"
        platform = "macos, beos"
    "#;
    assert!(errors(bad_filter, Os::MacOs)[0].contains("unknown platform \"beos\""));
    let not_keys = r#"
        [[bindings]]
        keys = 3
        command = "app.save"
    "#;
    assert!(errors(not_keys, Os::MacOs)[0].contains("a table of keys by platform"));
}

#[test]
fn filters_parse_and_match() {
    let matches = |source: &str| {
        let filter = PlatformFilter::parse(source).unwrap();
        Os::ALL.map(|os| filter.matches(os))
    };
    assert_eq!(matches("macos"), [true, false, false]);
    assert_eq!(matches("!macos"), [false, true, true]);
    assert_eq!(matches("linux, windows"), [false, true, true]);
    assert_eq!(matches("!linux, !windows"), [true, false, false]);
    assert!(PlatformFilter::parse("").is_err());
    assert_eq!(
        PlatformFilter::parse("linux, !macos").unwrap().to_string(),
        "linux, !macos"
    );
}

const MENUS: &str = r#"
[[menus]]
name = "Notes"
platform = "macos"

[[menus.items]]
title = "Hide Notes"
command = "app.hide"

[[menus]]
name = "File"

[[menus.items]]
title = "New"
command = "notes.new"

[[menus.items]]
separator = true
platform = "!macos"

[[menus.items]]
title = "Quit"
command = "app.quit"
platform = "!macos"
"#;

/// Each menu's name and its items' IDs, `-` for a separator.
fn menus(os: Os) -> Vec<(String, Vec<String>)> {
    let (bar, diagnostics) = MenuBar::parse_for(Path::new("menus.toml"), MENUS, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    bar.unwrap()
        .menus
        .into_iter()
        .map(|menu| {
            let items = menu
                .items
                .iter()
                .map(|item| match &item.kind {
                    ItemKind::Separator => "-".to_owned(),
                    _ => item.id().unwrap_or("?").to_owned(),
                })
                .collect();
            (menu.name, items)
        })
        .collect()
}

#[test]
fn menus_and_items_can_be_for_some_platforms() {
    let own = |name: &str, items: &[&str]| {
        (
            name.to_owned(),
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        menus(Os::MacOs),
        [own("Notes", &["app.hide"]), own("File", &["notes.new"])]
    );
    assert_eq!(
        menus(Os::Linux),
        [own("File", &["notes.new", "-", "app.quit"])]
    );
    let bad = "[[menus]]\nname = \"File\"\nplatform = \"plan9\"\n[[menus.items]]\ntitle = \"New\"\ncommand = \"notes.new\"\n";
    let (_, diagnostics) = MenuBar::parse_for(Path::new("m.toml"), bad, None, Os::Linux);
    assert!(
        diagnostics[0]
            .message
            .contains("unknown platform \"plan9\""),
        "{diagnostics:?}"
    );
}

#[test]
fn menu_shortcuts_resolve_in_the_apps_context() {
    let keymap = r#"
[[bindings]]
keys = "ctrl-n"
command = "notes.new"
context = "notes"

[[bindings]]
keys = "ctrl-q"
command = "app.quit"
"#;
    let (layer, diagnostics) = layer(keymap, Os::Linux);
    assert_eq!(diagnostics, []);
    let keymap = crate::Keymap::from_layers([layer.unwrap()]);
    let menus = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New"
command = "notes.new"

[[menus.items]]
title = "New Elsewhere"
id = "elsewhere"
command = "notes.new"
shortcut_context = ["other"]

[[menus.items]]
title = "Quit"
command = "app.quit"
"#;
    let shortcuts = |context: &[&str]| -> Vec<Option<String>> {
        let path = Path::new("menus.toml");
        let (bar, diagnostics) = MenuBar::parse_in(path, menus, None, Os::Linux, context);
        assert_eq!(diagnostics, []);
        let bar = bar.unwrap();
        bar.menus[0]
            .items
            .iter()
            .filter_map(|item| item.command())
            .map(|item| item.shortcut(&keymap).map(|keys| keys.to_string()))
            .collect()
    };
    let (new, quit) = (Some("ctrl-n".to_owned()), Some("ctrl-q".to_owned()));
    // By default, no frames: only the binding without a context shows.
    assert_eq!(shortcuts(&[]), [None, None, quit.clone()]);
    // The app's frames, unless an item names its own.
    assert_eq!(shortcuts(&["notes"]), [new, None, quit]);
    let (default, _) = MenuBar::parse(Path::new("menus.toml"), menus, None);
    let (none, _) = MenuBar::parse_in(Path::new("menus.toml"), menus, None, Os::current(), &[]);
    assert_eq!(default, none, "parse resolves in no frames");
}
