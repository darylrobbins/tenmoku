//! The bundled themes colour the highlights semantic tokens land on.

use std::path::Path;

use fe_config::theme::EXTENSIONS;
use kiln_core::theme::ThemeFile;

const THEMES: [(&str, &str); 4] = [
    ("dark", include_str!("../themes/fastedit-dark.toml")),
    ("light", include_str!("../themes/fastedit-light.toml")),
    ("ember", include_str!("../themes/fastedit-ember.toml")),
    (
        "vscode-dark-modern",
        include_str!("../themes/vscode-dark-modern.toml"),
    ),
];

#[test]
fn every_theme_colours_parameters_and_enum_members() {
    for (name, source) in THEMES {
        let (file, _) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
        let syntax = &file.unwrap().extensions["syntax"];
        for (key, parent) in [
            ("variable.parameter", None),
            ("type.parameter", Some("type")),
            ("constant.enum_member", Some("constant")),
        ] {
            let value = syntax
                .get(key)
                .unwrap_or_else(|| panic!("{name}: no {key}"));
            // VS Code colours a type parameter like its type, so Dark Modern's
            // `#4EC9B0` is the same for both.
            if let Some(parent) = parent.filter(|_| name != "vscode-dark-modern") {
                assert_ne!(Some(value), syntax.get(parent), "{name}: {key}");
            }
        }
    }
}
