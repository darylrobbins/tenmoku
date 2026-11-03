//! `vim.subword_motions`: off by default, settable from a user or a project
//! file, and in the schema.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

fn load(user: Option<&str>, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, contents: Option<&str>| {
        contents.map(|contents| {
            let path = dir.path().join(name);
            std::fs::write(&path, contents).unwrap();
            path
        })
    };
    let user = write("user.toml", user);
    let project = write("project.toml", project);
    SettingsStore::load(user, project)
}

#[test]
fn vim_subword_motions_setting() {
    assert!(!Settings::default().vim.subword_motions);

    let store = load(Some("[vim]\nsubword_motions = true\n"), None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().vim.subword_motions);

    let store = load(None, Some("[vim]\nsubword_motions = true\n"));
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().vim.subword_motions);

    let schema = fe_config::settings_schema().to_string();
    assert!(schema.contains("\"subword_motions\""));

    let store = load(Some(USER_SETTINGS_TEMPLATE), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
