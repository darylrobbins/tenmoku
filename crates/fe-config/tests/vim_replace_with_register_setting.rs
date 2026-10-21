//! `vim.replace_with_register`: off by default, settable from a user or a
//! project file, and in the schema.

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
fn vim_replace_with_register_setting() {
    assert!(!Settings::default().vim.replace_with_register);

    let store = load(Some("[vim]\nreplace_with_register = true\n"), None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().vim.replace_with_register);

    let store = load(None, Some("[vim]\nreplace_with_register = true\n"));
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().vim.replace_with_register);

    let schema = fe_config::settings_schema().to_string();
    assert!(schema.contains("\"replace_with_register\""));
    assert!(schema.contains("ReplaceWithRegister"));

    let store = load(Some(USER_SETTINGS_TEMPLATE), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
