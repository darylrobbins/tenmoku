//! `theme.dark` and `theme.light`: the installed themes the pair starts
//! from, unset by default, and the app's, so a project file can't set
//! them.

use std::fs;

use fe_config::{Settings, SettingsStore, Severity, settings::PROJECT_THEME};

#[test]
fn theme_names_key() {
    let defaults = Settings::default();
    assert_eq!(defaults.theme.dark, None);
    assert_eq!(defaults.theme.light, None);
    let dir = tempfile::tempdir().unwrap();

    let user = dir.path().join("settings.toml");
    fs::write(
        &user,
        "[theme]\ndark = \"acme.solarized/solarized-dark\"\nlight = \"acme.solarized/solarized-light\"\n",
    )
    .unwrap();
    let store = SettingsStore::load(Some(user.clone()), None);
    assert_eq!(store.diagnostics(), []);
    let theme = &store.settings().theme;
    assert_eq!(theme.dark.as_deref(), Some("acme.solarized/solarized-dark"));
    assert_eq!(
        theme.light.as_deref(),
        Some("acme.solarized/solarized-light")
    );

    let project = dir.path().join("project.toml");
    fs::write(
        &project,
        "[theme]\ndark = \"evil.theme/dark\"\nlight = \"\"\n",
    )
    .unwrap();
    let store = SettingsStore::load(Some(user), Some(project));
    let theme = &store.settings().theme;
    assert_eq!(theme.dark.as_deref(), Some("acme.solarized/solarized-dark"));
    assert_eq!(
        theme.light.as_deref(),
        Some("acme.solarized/solarized-light")
    );
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (Severity::Warning, Some("theme.dark"), 2, PROJECT_THEME),
            (Severity::Warning, Some("theme.light"), 3, PROJECT_THEME),
        ]
    );

    // An empty name isn't one.
    let user = dir.path().join("empty.toml");
    fs::write(&user, "[theme]\ndark = \"\"\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.line))
        .collect();
    assert_eq!(found, [(Severity::Error, 2)], "{:#?}", store.diagnostics());
    assert_eq!(store.settings().theme.dark, None);
}
