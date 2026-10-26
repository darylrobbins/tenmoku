//! `vim.preset`: `"none"` by default, or `"lazyvim"`.

use fe_config::{Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, VimPreset};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_preset_setting() {
    assert_eq!(Settings::default().vim.preset, VimPreset::None);

    let store = load("[vim]\npreset = \"none\"\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.preset, VimPreset::None);

    let store = load("[vim]\npreset = \"lazyvim\"\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.preset, VimPreset::Lazyvim);
    // The preset leaves `vim.leader` as written.
    assert_eq!(store.settings().vim.leader, "\\");

    let store = load("[vim]\nleader = \",\"\npreset = \"spacevim\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    // Located at the `preset` value.
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (3, 10));
    assert_eq!(store.settings().vim.preset, VimPreset::None);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
