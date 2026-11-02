//! `vim.quick_scope`: `"on_keys"` by default, `"off"` or `"always"`.

use fe_config::{
    Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings::VimQuickScope, settings_schema,
};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn defaults_to_on_keys() {
    assert_eq!(Settings::default().vim.quick_scope, VimQuickScope::OnKeys);
    let store = load("");
    assert_eq!(store.settings().vim.quick_scope, VimQuickScope::OnKeys);
}

#[test]
fn loads_each_value() {
    for (text, want) in [
        ("off", VimQuickScope::Off),
        ("on_keys", VimQuickScope::OnKeys),
        ("always", VimQuickScope::Always),
    ] {
        let store = load(&format!("[vim]\nquick_scope = \"{text}\"\n"));
        assert_eq!(store.diagnostics(), []);
        assert_eq!(store.settings().vim.quick_scope, want);
    }
}

#[test]
fn another_value_is_a_diagnostic() {
    let store = load("[vim]\nquick_scope = \"sometimes\"\n");
    assert_eq!(store.diagnostics().len(), 1);
    assert_eq!(store.settings().vim.quick_scope, VimQuickScope::OnKeys);
}

#[test]
fn the_template_and_schema_describe_it() {
    assert!(USER_SETTINGS_TEMPLATE.contains("# quick_scope = \"on_keys\"\n"));
    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
    let schema = settings_schema();
    let property = &schema["$defs"]["VimSettings"]["properties"]["quick_scope"];
    assert_eq!(property["default"], "on_keys");
    assert!(property["description"].as_str().unwrap().contains("`f`"));
}
