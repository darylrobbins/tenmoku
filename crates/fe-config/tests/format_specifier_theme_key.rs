//! Every bundled theme colours `string.placeholder` apart from `string`.

use std::path::Path;

use fe_config::theme::EXTENSIONS;
use kiln_core::theme::ThemeFile;

const THEMES: [(&str, &str); 6] = [
    ("dark", include_str!("../themes/tenmoku-dark.toml")),
    ("light", include_str!("../themes/tenmoku-light.toml")),
    ("ember", include_str!("../themes/tenmoku-ember.toml")),
    (
        "vscode-dark-modern",
        include_str!("../themes/vscode-dark-modern.toml"),
    ),
    (
        "vscode-2026-dark",
        include_str!("../themes/vscode-2026-dark.toml"),
    ),
    (
        "vscode-2026-light",
        include_str!("../themes/vscode-2026-light.toml"),
    ),
];

#[test]
fn every_theme_colours_placeholders_apart_from_strings() {
    for (name, source) in THEMES {
        let (file, _) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
        let syntax = &file.unwrap().extensions["syntax"];
        let placeholder = syntax
            .get("string.placeholder")
            .unwrap_or_else(|| panic!("{name}: no string.placeholder"));
        assert_ne!(Some(placeholder), syntax.get("string"), "{name}");
    }
}
