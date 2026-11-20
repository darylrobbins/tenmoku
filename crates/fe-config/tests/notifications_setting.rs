//! `workbench.notifications`: `all`, `needs_you` or `off`, `all` by default,
//! and the user's alone.

use fe_config::{NotificationLevel, Settings, SettingsStore};

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|text| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, text).unwrap();
        path
    });
    SettingsStore::load(Some(user_path), project_path)
}

#[test]
fn the_level_parses_and_defaults_to_all() {
    assert_eq!(
        Settings::default().workbench.notifications,
        NotificationLevel::All
    );
    for (name, want) in [
        ("all", NotificationLevel::All),
        ("needs_you", NotificationLevel::NeedsYou),
        ("off", NotificationLevel::Off),
    ] {
        let store = load(&format!("[workbench]\nnotifications = \"{name}\"\n"), None);
        assert_eq!(store.diagnostics(), [], "{name}");
        assert_eq!(store.settings().workbench.notifications, want, "{name}");
    }
    let store = load("[workbench]\nnotifications = \"loud\"\n", None);
    assert_eq!(store.diagnostics().len(), 1);
    assert_eq!(
        store.settings().workbench.notifications,
        NotificationLevel::All
    );
}

#[test]
fn a_project_file_cannot_set_it() {
    let store = load("", Some("[workbench]\nnotifications = \"off\"\n"));
    assert_eq!(
        store.settings().workbench.notifications,
        NotificationLevel::All
    );
    assert_eq!(store.diagnostics().len(), 1, "{:#?}", store.diagnostics());
}
