// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`]: Lucide's icons, each in its file type's or folder role's
//! `file_icon` token, which follows `list.icon` under a theme that sets none.

use std::{collections::BTreeSet, path::Path, sync::Arc};

use gpui::{TestAppContext, rgb};
use kiln_core::theme::{TOKEN_KEYS, ThemeFile};

use crate::{
    ActiveTheme as _, Icon, IconRef, IconSet, LucideIcons, Theme, ThemeData, TintedIcons,
    file_icon_ref, file_icons_tinted::*, folder_icon_ref_named, set_icon_set, set_theme,
};

/// A theme setting each `file_icon` token to a colour of its own.
fn tinted_theme() -> Theme {
    let tokens = file_icon_keys()
        .iter()
        .enumerate()
        .map(|(i, key)| {
            let name = key.strip_prefix("file_icon.").unwrap();
            format!("{name} = \"#{:02x}2040\"\n", 0x30 + i)
        })
        .collect::<String>();
    let source = format!(
        "kiln_theme = 1\nname = \"Tinted\"\nappearance = \"dark\"\n\n[file_icon]\n{tokens}"
    );
    let (file, diagnostics) = ThemeFile::parse(Path::new("tinted.toml"), &source, &[]);
    assert_eq!(diagnostics, []);
    Theme::new(ThemeData::from_file(&file.unwrap()))
}

fn file_icon_keys() -> Vec<&'static str> {
    (TOKEN_KEYS.iter().copied())
        .filter(|key| key.starts_with("file_icon."))
        .collect()
}

#[gpui::test]
fn types_and_folder_roles_take_their_tint(cx: &mut TestAppContext) {
    let set = TintedIcons;
    // Each file takes Lucide's icon, in its type's token.
    for (name, tint) in [
        ("main.rs", RUST),
        ("Cargo.toml", TOML),
        ("config.toml", TOML),
        ("notes.md", MARKDOWN),
        ("README.md", MARKDOWN),
        ("settings.yaml", CONFIG),
        ("Makefile", CONFIG),
        (".env.local", CONFIG),
        ("package.json", DATA),
        ("Cargo.lock", DATA),
        ("build.sh", SCRIPT),
        ("app.tsx", WEB),
        ("logo.png", IMAGE),
        ("main.go", DEFAULT),
        ("unknown", DEFAULT),
    ] {
        let icon = LucideIcons.file(name).bundled().unwrap();
        assert_eq!(set.file(name), IconRef::Tinted(icon, tint), "{name}");
    }
    // A README is the Markdown icon, whatever its extension.
    for name in ["README", "readme.txt", "README.md"] {
        assert_eq!(
            set.file(name),
            IconRef::Tinted(Icon::FileText, MARKDOWN),
            "{name}"
        );
    }
    // Folders by role, open or closed; any other is the plain folder.
    for (name, tint) in [
        ("src", FOLDER_SOURCE),
        ("crates", FOLDER_SOURCE),
        ("tests", FOLDER_TESTS),
        ("__tests__", FOLDER_TESTS),
        ("docs", FOLDER_DOCS),
        ("Docs", FOLDER_DOCS),
        ("node_modules", FOLDER),
    ] {
        for expanded in [false, true] {
            let icon = LucideIcons.folder(expanded).bundled().unwrap();
            let want = IconRef::Tinted(icon, tint);
            assert_eq!(set.folder_named(name, expanded), want, "{name}");
        }
    }
    assert_eq!(set.folder(true), IconRef::Tinted(Icon::FolderOpen, FOLDER));
    // Drawn in its own colour, it dims as a coloured icon does.
    assert!(set.file("main.rs").is_coloured());

    // Installed, every surface asks it.
    cx.update(|cx| set_icon_set(Arc::new(TintedIcons), cx));
    assert_eq!(
        file_icon_ref("lib.rs"),
        IconRef::Tinted(Icon::FileCode, RUST)
    );
    assert_eq!(
        folder_icon_ref_named("docs", false),
        IconRef::Tinted(Icon::Folder, FOLDER_DOCS)
    );

    // Under a theme that sets them, each draws in its token's colour,
    // whatever tint it's drawn with.
    let tinted = tinted_theme();
    cx.update(|cx| set_theme(cx, tinted.clone()));
    let fallback = rgb(0xff0000).into();
    let mut colours = BTreeSet::new();
    for key in file_icon_keys() {
        let want = *tinted.colors.get(key).unwrap();
        let drawn = IconRef::Tinted(Icon::File, key).tint(fallback);
        assert_eq!(drawn, want, "{key}");
        colours.insert(format!("{drawn:?}"));
    }
    assert_eq!(colours.len(), file_icon_keys().len(), "one colour each");
    assert_eq!(
        set.file("main.rs").tint(fallback),
        tinted.colors.file_icon.rust
    );
    assert_eq!(
        set.folder_named("tests", false).tint(fallback),
        tinted.colors.file_icon.folder_tests
    );
    // An untinted icon keeps the tint it's drawn with.
    assert_eq!(IconRef::Bundled(Icon::File).tint(fallback), fallback);

    // A theme that sets none draws every one in list.icon.
    for theme in [Theme::dark(), Theme::light()] {
        cx.update(|cx| set_theme(cx, theme.clone()));
        let icon = cx.read(|cx| cx.theme().colors.list.icon);
        assert_eq!(icon, theme.colors.list.icon);
        for key in file_icon_keys() {
            let drawn = IconRef::Tinted(Icon::File, key).tint(fallback);
            assert_eq!(drawn, icon, "{}: {key}", theme.name);
        }
        for name in ["main.rs", "Cargo.toml", "README.md"] {
            assert_eq!(set.file(name).tint(fallback), icon, "{name}");
        }
        assert_eq!(set.folder_named("src", true).tint(fallback), icon);
    }
}

#[test]
fn tables_name_file_icon_tokens() {
    let keys = file_icon_keys();
    let named: BTreeSet<&str> = (FILE_NAMES.iter().chain(EXTENSIONS).chain(FOLDER_NAMES))
        .map(|&(_, tint)| tint)
        .chain([DEFAULT, FOLDER])
        .collect();
    // Every token is one the tables or the defaults name, and back.
    assert_eq!(
        named,
        keys.iter().copied().collect(),
        "the file_icon tokens"
    );
    // Lowercase keys, each once, so a lookup can find them.
    for table in [FILE_NAMES, EXTENSIONS, FOLDER_NAMES] {
        let mut seen = BTreeSet::new();
        for &(key, _) in table {
            assert_eq!(key, key.to_ascii_lowercase(), "{key}");
            assert!(seen.insert(key), "{key} twice");
        }
    }
}
