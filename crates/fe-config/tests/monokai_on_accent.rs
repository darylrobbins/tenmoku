//! Monokai's primary-button text reads on its accent fill.

use std::path::Path;

use fe_config::{theme::EXTENSIONS, themes::find};
use kiln_core::theme::{Color, ThemeData, ThemeFile};

/// WCAG relative luminance of the opaque colour `0xrrggbbaa`.
fn luminance(color: Color) -> f64 {
    let channel = |shift: u32| {
        let c = f64::from((color.0 >> shift) & 0xFF) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(24) + 0.7152 * channel(16) + 0.0722 * channel(8)
}

#[test]
fn monokai_primary_button_text_is_readable() {
    let theme = find("vscode-monokai").expect("Monokai is bundled");
    let (file, diagnostics) =
        ThemeFile::parse(Path::new("vscode-monokai.toml"), theme.source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let data = ThemeData::from_file(&file.unwrap());
    let text = data.tokens.text.on_accent;
    let fill = data.tokens.accent.base;
    assert_eq!(text, Color::rgb(0xF8F8F2));
    let (a, b) = (luminance(text), luminance(fill));
    let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
    assert!(ratio >= 4.5, "contrast {ratio:.2}:1 is under 4.5:1");
}
