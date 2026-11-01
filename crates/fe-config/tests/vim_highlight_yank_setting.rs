//! `vim.highlight_yank` and `vim.highlight_yank_ms`.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn highlight_yank_defaults_round_trip_and_schema() {
    let vim = Settings::default().vim;
    assert!(vim.highlight_yank);
    assert_eq!(vim.highlight_yank_ms, 150);

    let store = load("[vim]\nhighlight_yank = false\nhighlight_yank_ms = 400\n");
    assert_eq!(store.diagnostics(), []);
    let vim = &store.settings().vim;
    assert!(!vim.highlight_yank);
    assert_eq!(vim.highlight_yank_ms, 400);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    let schema = settings_schema();
    let props = &schema["$defs"]["VimSettings"]["properties"];
    assert_eq!(props["highlight_yank"]["type"], "boolean");
    assert_eq!(props["highlight_yank"]["default"], true);
    assert_eq!(props["highlight_yank_ms"]["default"], 150);
    assert_eq!(props["highlight_yank_ms"]["minimum"], 1);
    assert_eq!(props["highlight_yank_ms"]["maximum"], 5000);
    for key in ["highlight_yank", "highlight_yank_ms"] {
        assert!(USER_SETTINGS_TEMPLATE.contains(key), "{key}");
    }
}
