//! `terminal.font_fallbacks`: empty by default, a list of strings from user
//! and project files, and a non-list value keeping the default.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|contents| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, contents).unwrap();
        path
    });
    SettingsStore::load(Some(user_path), project_path)
}

#[test]
fn defaults_to_empty() {
    assert!(Settings::default().terminal.font_fallbacks.is_empty());
    let store = load(USER_SETTINGS_TEMPLATE, None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().terminal.font_fallbacks.is_empty());
}

#[test]
fn a_user_file_sets_the_list() {
    let store = load(
        "[terminal]\nfont_fallbacks = [\"Symbols Nerd Font Mono\", \"Apple Color Emoji\"]\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    assert_eq!(
        store.settings().terminal.font_fallbacks,
        ["Symbols Nerd Font Mono", "Apple Color Emoji"]
    );
}

#[test]
fn a_project_file_sets_the_list() {
    let store = load(
        "[terminal]\nfont_fallbacks = [\"A\"]\n",
        Some("[terminal]\nfont_fallbacks = [\"B\"]\n"),
    );
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().terminal.font_fallbacks, ["B"]);
}

#[test]
fn a_non_list_keeps_the_default() {
    for source in [
        "[terminal]\nfont_fallbacks = \"Symbols Nerd Font Mono\"\n",
        "[terminal]\nfont_fallbacks = [1, 2]\n",
    ] {
        let store = load(source, None);
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
        assert!(
            store.settings().terminal.font_fallbacks.is_empty(),
            "{source}"
        );
    }
}
