// SPDX-License-Identifier: GPL-3.0-or-later
//! `property` has a colour of its own in Hakuji, Yohen and the Catppuccin
//! themes: not the text colour, not the variable colour.

use std::path::Path;

use fe_config::theme::EXTENSIONS;
use kiln_core::theme::ThemeFile;

const THEMES: [(&str, &str); 6] = [
    ("hakuji", include_str!("../themes/hakuji.toml")),
    ("yohen", include_str!("../themes/yohen.toml")),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.toml"),
    ),
    (
        "catppuccin-macchiato",
        include_str!("../themes/catppuccin-macchiato.toml"),
    ),
    (
        "catppuccin-frappe",
        include_str!("../themes/catppuccin-frappe.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.toml"),
    ),
];

fn parse(name: &str, source: &str) -> kiln_core::theme::ThemeFile {
    let (file, _) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    file.unwrap_or_else(|| panic!("{name}: does not parse"))
}

#[test]
fn property_is_not_plain_text_in_listed_themes() {
    for (name, source) in THEMES {
        let file = parse(name, source);
        let syntax = &file.extensions["syntax"];
        let property = syntax
            .get("property")
            .unwrap_or_else(|| panic!("{name}: no property"));
        let text = file.extensions["editor"]
            .get("foreground")
            .unwrap_or_else(|| panic!("{name}: no editor.foreground"));
        assert_ne!(property, text, "{name}: property is the text colour");
        if let Some(variable) = syntax.get("variable") {
            assert_ne!(property, variable, "{name}: property is variable's");
        }
    }
}

#[test]
fn hakuji_and_yohen_property_is_the_key_colour() {
    for (name, source) in &THEMES[..2] {
        let file = parse(name, source);
        let syntax = &file.extensions["syntax"];
        assert_eq!(
            syntax.get("property"),
            syntax.get("string.special.key"),
            "{name}"
        );
    }
}
