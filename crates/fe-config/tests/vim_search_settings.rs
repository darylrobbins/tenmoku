//! `vim.ignorecase`, `vim.smartcase` and `vim.incsearch`.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_search_settings_defaults_and_round_trip() {
    let vim = Settings::default().vim;
    assert!(!vim.ignorecase);
    assert!(!vim.smartcase);
    assert!(vim.incsearch);

    let store = load("[vim]\nignorecase = true\nsmartcase = true\nincsearch = false\n");
    assert_eq!(store.diagnostics(), []);
    let vim = &store.settings().vim;
    assert!(vim.ignorecase && vim.smartcase && !vim.incsearch);

    let json = serde_json::to_string(&store.settings().vim).unwrap();
    let back: fe_config::VimSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, vim);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    let schema = settings_schema();
    let props = &schema["$defs"]["VimSettings"]["properties"];
    for (key, default) in [
        ("ignorecase", false),
        ("smartcase", false),
        ("incsearch", true),
    ] {
        assert_eq!(props[key]["type"], "boolean", "{key}");
        assert_eq!(props[key]["default"], default, "{key}");
        assert!(
            props[key]["description"]
                .as_str()
                .is_some_and(|d| !d.is_empty()),
            "{key}"
        );
    }
}
