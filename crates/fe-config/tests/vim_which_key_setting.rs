//! `vim.which_key` and `vim.which_key_delay_ms`: on, after 200 ms, by
//! default; the delay is 0 to 5000.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_which_key_setting() {
    let defaults = Settings::default().vim;
    assert!(defaults.which_key);
    assert_eq!(defaults.which_key_delay_ms, 200);

    let store = load("[vim]\nwhich_key = false\nwhich_key_delay_ms = 0\n");
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().vim.which_key);
    assert_eq!(store.settings().vim.which_key_delay_ms, 0);

    let store = load("[vim]\nwhich_key_delay_ms = 5000\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.which_key_delay_ms, 5000);

    // Out of range: an error at the value, and the default kept.
    let store = load("[vim]\nwhich_key_delay_ms = 5001\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
    assert_eq!(
        diagnostics[0].key.as_deref(),
        Some("vim.which_key_delay_ms")
    );
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (2, 22));
    assert_eq!(store.settings().vim.which_key_delay_ms, 200);

    let schema = settings_schema();
    let vim = &schema["$defs"]["VimSettings"]["properties"];
    assert_eq!(vim["which_key"]["type"], "boolean");
    assert_eq!(vim["which_key_delay_ms"]["minimum"], 0);
    assert_eq!(vim["which_key_delay_ms"]["maximum"], 5000);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
    assert!(USER_SETTINGS_TEMPLATE.contains("# which_key = true\n"));
    assert!(USER_SETTINGS_TEMPLATE.contains("# which_key_delay_ms = 200\n"));
}
