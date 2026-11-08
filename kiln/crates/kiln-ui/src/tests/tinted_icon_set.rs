// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`]: line icons, a glyph per language, each in its file
//! type's or folder role's `file_icon` token, which follows `list.icon` under
//! a theme that sets none.

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
    // Each file takes its language's glyph, or Lucide's icon where it has
    // none, in its type's token.
    for (name, icon, tint) in [
        ("main.rs", Icon::BrandRust, RUST),
        ("main.go", Icon::BrandGolang, CODE),
        ("notes.md", Icon::Markdown, MARKDOWN),
        ("settings.yaml", Icon::FileSliders, CONFIG),
        ("Makefile", Icon::Hammer, CONFIG),
        ("build.sh", Icon::FileTerminal, SCRIPT),
        ("app.tsx", Icon::BrandReact, WEB),
        ("Cargo.toml", Icon::Package, RUST),
        ("package.json", Icon::BrandNodejs, WEB),
    ] {
        assert_eq!(set.file(name), IconRef::Tinted(icon, tint), "{name}");
    }
    for (name, tint) in [
        ("config.toml", TOML),
        (".env.local", CONFIG),
        ("Cargo.lock", DATA),
        ("logo.png", IMAGE),
        ("unknown", DEFAULT),
    ] {
        let icon = LucideIcons.file(name).bundled().unwrap();
        assert_eq!(set.file(name), IconRef::Tinted(icon, tint), "{name}");
    }
    // A README is the Markdown icon, whatever its extension.
    for name in ["README", "readme.txt", "README.md"] {
        assert_eq!(
            set.file(name),
            IconRef::Tinted(Icon::Markdown, MARKDOWN),
            "{name}"
        );
    }
    // Folders by role, a glyph each, the same open or closed; any other is
    // the plain folder.
    for (name, icon, tint) in [
        ("src", Icon::FolderCode, FOLDER_SOURCE),
        ("crates", Icon::FolderCode, FOLDER_SOURCE),
        ("tests", Icon::FolderCheck, FOLDER_TESTS),
        ("__tests__", Icon::FolderCheck, FOLDER_TESTS),
        ("docs", Icon::FolderPen, FOLDER_DOCS),
        ("Docs", Icon::FolderPen, FOLDER_DOCS),
        ("node_modules", Icon::FolderArchive, FOLDER),
    ] {
        for expanded in [false, true] {
            let want = IconRef::Tinted(icon, tint);
            assert_eq!(set.folder_named(name, expanded), want, "{name}");
        }
    }
    for expanded in [false, true] {
        let icon = LucideIcons.folder(expanded).bundled().unwrap();
        let want = IconRef::Tinted(icon, FOLDER);
        assert_eq!(set.folder_named("demo-app", expanded), want);
    }
    assert_eq!(set.folder(true), IconRef::Tinted(Icon::FolderOpen, FOLDER));
    // Drawn in its own colour, it dims as a coloured icon does.
    assert!(set.file("main.rs").is_coloured());

    // Installed, every surface asks it.
    cx.update(|cx| set_icon_set(Arc::new(TintedIcons), cx));
    assert_eq!(
        file_icon_ref("lib.rs"),
        IconRef::Tinted(Icon::BrandRust, RUST)
    );
    assert_eq!(
        folder_icon_ref_named("docs", false),
        IconRef::Tinted(Icon::FolderPen, FOLDER_DOCS)
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
    let named: BTreeSet<&str> = (FILE_NAMES.iter().chain(EXTENSIONS))
        .map(|&(_, tint)| tint)
        .chain(FOLDER_NAMES.iter().map(|&(_, role)| role.tint()))
        .chain([DEFAULT, FOLDER])
        .collect();
    // Every token is one the tables or the defaults name, and back.
    assert_eq!(
        named,
        keys.iter().copied().collect(),
        "the file_icon tokens"
    );
    // Lowercase keys, each once, so a lookup can find them.
    for table in [FILE_NAMES, EXTENSIONS] {
        lowercase_and_once(table.iter().map(|&(key, _)| key));
    }
    lowercase_and_once(FOLDER_NAMES.iter().map(|&(key, _)| key));
    for table in [ICON_NAMES, ICON_EXTENSIONS] {
        lowercase_and_once(table.iter().map(|&(key, _)| key));
    }
}

fn lowercase_and_once<'a>(keys: impl Iterator<Item = &'a str>) {
    let mut seen = BTreeSet::new();
    for key in keys {
        assert_eq!(key, key.to_ascii_lowercase(), "{key}");
        assert!(seen.insert(key), "{key} twice");
    }
}
