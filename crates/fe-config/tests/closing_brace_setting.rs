//! `editor.closing_brace_hints` (on) and `editor.closing_brace_hints_min_rows` (25).

use fe_config::{Settings, SettingsStore};

#[test]
fn defaults_are_on_and_25() {
    let editor = Settings::default().editor;
    assert!(editor.closing_brace_hints);
    assert_eq!(editor.closing_brace_hints_min_rows, 25);
}

#[test]
fn the_keys_load() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(
        &user,
        "[editor]\nclosing_brace_hints = false\nclosing_brace_hints_min_rows = 10\n",
    )
    .unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().editor.closing_brace_hints);
    assert_eq!(store.settings().editor.closing_brace_hints_min_rows, 10);
}
