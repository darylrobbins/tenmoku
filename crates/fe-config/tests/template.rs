//! The settings template a new user starts from.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

#[test]
fn template_is_all_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, USER_SETTINGS_TEMPLATE).unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(**store.settings(), Settings::default());
    assert_eq!(store.diagnostics(), []);

    let defaults = toml::Table::try_from(Settings::default()).unwrap();
    let editor = defaults["editor"].as_table().unwrap();
    let mut keys: Vec<_> = editor.keys().cloned().collect();
    // `font_family` is unset by default, so it isn't serialized.
    keys.push("font_family".to_owned());
    keys.sort();
    assert_eq!(
        keys,
        [
            "font_family",
            "font_size",
            "line_height",
            "soft_wrap",
            "tab_size"
        ]
    );
    for key in keys {
        let value = match key.as_str() {
            "font_family" => "\"JetBrains Mono\"".to_owned(),
            _ => editor[&key].to_string(),
        };
        let line = format!("# {key} = ");
        let found = USER_SETTINGS_TEMPLATE
            .lines()
            .find(|l| l.starts_with(&line))
            .unwrap_or_else(|| panic!("{key} is commented out in the template"));
        if key != "font_family" {
            assert_eq!(found[line.len()..], value, "{key}'s default");
        }
    }
}
