//! `editor.wrap_column` and `editor.wrap_mode`.

use fe_config::{
    Settings, SettingsStore, USER_SETTINGS_TEMPLATE,
    settings::{EditorSettings, WrapMode},
    settings_schema,
};

fn load(contents: &str) -> (SettingsStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    (SettingsStore::load(Some(user), None), dir)
}

fn editor(store: &SettingsStore) -> &EditorSettings {
    &store.settings().editor
}

#[test]
fn wrap_settings_default_and_validate() {
    let defaults = Settings::default().editor;
    assert_eq!(defaults.wrap_column, 0);
    assert_eq!(defaults.wrap_mode, WrapMode::Viewport);
    assert_eq!(defaults.effective_wrap_column(), None);

    let (store, _dir) = load("[editor]\nwrap_column = 80\nwrap_mode = \"bounded\"\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(editor(&store).wrap_column, 80);
    assert_eq!(editor(&store).effective_wrap_column(), Some(80));
    assert_eq!(editor(&store).wrap_mode, WrapMode::Bounded);

    // Out of range: rejected with a diagnostic or clamped to 20..=500.
    for (value, clamped) in [(10, 20), (600, 500)] {
        let (store, _dir) = load(&format!("[editor]\nwrap_column = {value}\n"));
        let editor = editor(&store);
        let column = editor.effective_wrap_column();
        assert!(
            column == Some(clamped) || (!store.diagnostics().is_empty() && column.is_none()),
            "{value}: {column:?} {:?}",
            store.diagnostics()
        );
    }

    let (store, _dir) = load("[editor]\nwrap_mode = \"sideways\"\n");
    assert!(!store.diagnostics().is_empty());
    assert_eq!(editor(&store).wrap_mode, WrapMode::Viewport);

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# wrap_column = 0\n"));
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# wrap_mode = \"viewport\"\n"));
    let (store, _dir) = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    let schema = settings_schema();
    let properties = &schema["$defs"]["EditorSettings"]["properties"];
    assert_eq!(properties["wrap_column"]["default"], 0);
    let allowed = &properties["wrap_column"]["anyOf"];
    assert_eq!(allowed[0]["const"], 0);
    assert_eq!(allowed[1]["minimum"], 20);
    assert_eq!(allowed[1]["maximum"], 500);
    // The clamp stays for values the schema doesn't catch: 1 to 19 count as 20.
    let mut editor = EditorSettings::default();
    for (set, wanted) in [
        (0, None),
        (1, Some(20)),
        (19, Some(20)),
        (500, Some(500)),
        (501, Some(500)),
    ] {
        editor.wrap_column = set;
        assert_eq!(editor.effective_wrap_column(), wanted, "{set}");
    }
    assert_eq!(properties["wrap_mode"]["default"], "viewport");
    assert!(
        schema["$defs"]["WrapMode"].to_string().contains("bounded"),
        "{schema}"
    );
}
