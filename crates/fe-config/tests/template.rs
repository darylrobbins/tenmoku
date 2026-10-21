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
    // These are unset by default, so they aren't serialized.
    commented_out_with_defaults(&defaults, "editor", &["font_family"]);
    commented_out_with_defaults(&defaults, "editor.minimap", &[]);
    commented_out_with_defaults(&defaults, "terminal", &["font_family", "font_size"]);
}

/// The `[name]` section of the template comments out each of its settings
/// once, with its default, and nothing else: the keys `defaults` has and
/// `unset`. A dotted `name` is a nested table, such as `editor.minimap`,
/// whose section is its own; a table's nested tables aren't its settings.
fn commented_out_with_defaults(defaults: &toml::Table, name: &str, unset: &[&str]) {
    let table = name
        .split('.')
        .fold(defaults, |table, key| table[key].as_table().unwrap());
    let mut keys: Vec<&str> = table
        .iter()
        .filter(|(_, value)| !value.is_table())
        .map(|(key, _)| key.as_str())
        .chain(unset.iter().copied())
        .collect();
    keys.sort();

    let header = format!("[{name}]\n");
    let (_, section) = USER_SETTINGS_TEMPLATE.split_once(&header).unwrap();
    let section = section.split("\n[").next().unwrap();
    let commented: Vec<(&str, &str)> = section
        .lines()
        .filter_map(|line| line.strip_prefix("# ")?.split_once(" = "))
        .collect();
    let mut found: Vec<&str> = commented.iter().map(|&(key, _)| key).collect();
    found.sort();
    assert_eq!(found, keys, "[{name}]'s commented-out keys");

    for (key, value) in commented {
        if let Some(default) = table.get(key) {
            assert_eq!(value, default.to_string(), "{name}.{key}'s default");
        }
    }
}
