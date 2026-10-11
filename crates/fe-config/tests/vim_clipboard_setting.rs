//! `vim.clipboard`: `""` by default, or `"unnamedplus"`.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings::VimClipboard};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_clipboard_setting() {
    assert_eq!(Settings::default().vim.clipboard, VimClipboard::Off);

    let store = load("[vim]\nclipboard = \"unnamedplus\"\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.clipboard, VimClipboard::Unnamedplus);

    let store = load("[vim]\nclipboard = \"bogus\"\n");
    assert_eq!(store.diagnostics().len(), 1);
    assert_eq!(store.settings().vim.clipboard, VimClipboard::Off);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
