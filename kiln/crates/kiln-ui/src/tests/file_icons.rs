// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use gpui::AssetSource;

use crate::{
    Assets, Icon, file_icon,
    file_icons::{EXTENSIONS, FILE_NAMES},
    folder_icon,
};

#[test]
fn names_map_by_name_then_extension() {
    let cases = [
        ("Cargo.toml", Icon::Package),
        ("Cargo.lock", Icon::FileLock),
        ("main.rs", Icon::FileCode),
        ("MAIN.RS", Icon::FileCode),
        ("README.md", Icon::FileText),
        ("data.json", Icon::FileBraces),
        ("archive.tar.gz", Icon::FileArchive),
        (".gitignore", Icon::GitBranch),
        ("Dockerfile", Icon::Container),
        (".env.local", Icon::FileKey),
        ("LICENSE", Icon::Scale),
        ("Makefile", Icon::FileCog),
        ("noext", Icon::File),
        ("trailing.", Icon::File),
        (".hidden", Icon::File),
    ];
    for (name, icon) in cases {
        assert_eq!(file_icon(name), icon, "{name}");
    }
    // A leading dot doesn't start an extension: `.rs` is a name, not `rs`.
    assert_eq!(file_icon(".rs"), Icon::File);
}

#[test]
fn folders_and_long_names() {
    assert_eq!(folder_icon(false), Icon::Folder);
    assert_eq!(folder_icon(true), Icon::FolderOpen);

    let long_rs = format!("{}.rs", "a".repeat(297));
    assert_eq!(long_rs.len(), 300);
    assert_eq!(file_icon(&long_rs), Icon::FileCode);
    let long_plain = "B".repeat(64);
    assert_eq!(file_icon(&long_plain), Icon::File);
    assert_eq!(file_icon(&"c".repeat(300)), Icon::File);

    for (table, entries) in [("FILE_NAMES", FILE_NAMES), ("EXTENSIONS", EXTENSIONS)] {
        let mut seen = HashSet::new();
        for &(key, _) in entries {
            assert!(
                !key.is_empty() && key.len() <= 64,
                "{table}: {key:?} can't be looked up"
            );
            assert_eq!(key, key.to_ascii_lowercase(), "{table}: {key:?}");
            assert!(key.is_ascii(), "{table}: {key:?}");
            assert!(seen.insert(key), "{table}: duplicate {key:?}");
        }
    }
}

fn assert_bundled_but_unmapped(expected: &[(Icon, &str)]) {
    for &(icon, path) in expected {
        assert_eq!(icon.path(), path);
        assert!(matches!(Assets.load(path), Ok(Some(_))), "{path}");
        assert!(
            !FILE_NAMES
                .iter()
                .chain(EXTENSIONS)
                .any(|&(_, mapped)| mapped == icon),
            "{icon:?} is mapped"
        );
    }
}

#[test]
fn rail_icons_are_bundled_but_unmapped() {
    assert_bundled_but_unmapped(&[
        (Icon::Files, "kiln-ui/icons/files.svg"),
        (Icon::Search, "kiln-ui/icons/search.svg"),
        (Icon::ListTree, "kiln-ui/icons/list-tree.svg"),
    ]);
}

#[test]
fn toolbar_icons_are_bundled_but_unmapped() {
    assert_bundled_but_unmapped(&[
        (Icon::ArrowLeft, "kiln-ui/icons/arrow-left.svg"),
        (Icon::ArrowRight, "kiln-ui/icons/arrow-right.svg"),
        (Icon::Columns2, "kiln-ui/icons/columns-2.svg"),
        (Icon::Rows2, "kiln-ui/icons/rows-2.svg"),
        (Icon::Ellipsis, "kiln-ui/icons/ellipsis.svg"),
    ]);
}

#[test]
fn window_button_icons_are_bundled_but_unmapped() {
    assert_bundled_but_unmapped(&[
        (Icon::Minus, "kiln-ui/icons/minus.svg"),
        (Icon::Square, "kiln-ui/icons/square.svg"),
        (Icon::Copy, "kiln-ui/icons/copy.svg"),
        (Icon::X, "kiln-ui/icons/x.svg"),
        (Icon::Check, "kiln-ui/icons/check.svg"),
    ]);
}
