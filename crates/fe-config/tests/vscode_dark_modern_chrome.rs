//! VS Code Dark Modern sets the window's chrome through kiln's token groups.

use std::path::Path;

use fe_config::theme::EXTENSIONS;
use kiln_core::theme::{Color, ThemeData, ThemeFile};

const SOURCE: &str = include_str!("../themes/vscode-dark-modern.toml");

fn data() -> ThemeData {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), SOURCE, EXTENSIONS);
    assert_eq!(diagnostics, []);
    ThemeData::from_file(&file.unwrap())
}

#[test]
fn chrome_matches_upstream() {
    let t = data().tokens;
    // dark_modern.json: sideBar/titleBar/statusBar/tab backgrounds, the
    // active tab, every `.border`, and button/focusBorder.
    assert_eq!(t.surface.panel, Color::rgb(0x181818));
    assert_eq!(t.surface.background, Color::rgb(0x1F1F1F));
    assert_eq!(t.border.default, Color::rgb(0x2B2B2B));
    assert_eq!(t.tab.inactive, Color::rgb(0x181818));
    assert_eq!(t.tab.active, Color::rgb(0x1F1F1F));
    assert_eq!(t.accent.base, Color::rgb(0x0078D4));
}

#[test]
fn chrome_differs_from_kiln_dark() {
    let t = data().tokens;
    assert_ne!(t.surface.panel, Color::rgb(0x121418));
}
