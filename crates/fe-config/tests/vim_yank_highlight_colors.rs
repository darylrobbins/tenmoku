//! The `vim_yank_highlight` editor colour.

use std::path::Path;

use fe_config::{EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

const BUNDLED: &[&str] = &[
    include_str!("../themes/tenmoku-dark.toml"),
    include_str!("../themes/tenmoku-light.toml"),
    include_str!("../themes/tenmoku-ember.toml"),
    include_str!("../themes/catppuccin-latte.toml"),
    include_str!("../themes/catppuccin-frappe.toml"),
    include_str!("../themes/catppuccin-macchiato.toml"),
    include_str!("../themes/catppuccin-mocha.toml"),
    include_str!("../themes/vscode-2026-dark.toml"),
    include_str!("../themes/vscode-2026-light.toml"),
    include_str!("../themes/vscode-dark-modern.toml"),
    include_str!("../themes/vscode-monokai.toml"),
];

fn parse(source: &str) -> ThemeData {
    let (file, _) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    ThemeData::from_file(&file.unwrap())
}

#[test]
fn every_bundled_theme_sets_it() {
    assert_eq!(
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/themes"))
            .unwrap()
            .count(),
        BUNDLED.len()
    );
    for source in BUNDLED {
        let (file, diagnostics) = ThemeFile::parse(Path::new("t.toml"), source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        assert!(file.unwrap().extensions["editor"].contains_key("vim_yank_highlight"));
    }
}

#[test]
fn a_user_theme_without_it_gets_the_bundled_one() {
    let theme = parse("appearance = \"dark\"\n\n[editor]\nbackground = \"#101010\"\n");
    let (palette, warnings) = EditorPalette::for_theme(&theme);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(
        palette.editor.vim_yank_highlight,
        EditorPalette::fastedit_dark().editor.vim_yank_highlight
    );
    let theme = parse("appearance = \"dark\"\n\n[editor]\nvim_yank_highlight = \"#11223344\"\n");
    let (palette, _) = EditorPalette::for_theme(&theme);
    assert_eq!(
        palette.editor.vim_yank_highlight,
        "#11223344".parse().unwrap()
    );
}
