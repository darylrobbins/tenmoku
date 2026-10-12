//! `editor.auto_save` and `editor.auto_save_delay`.

use fe_config::settings::AutoSave;
use fe_config::{Settings, SettingsStore};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn auto_save_defaults_and_parse() {
    let defaults = Settings::default();
    assert_eq!(defaults.editor.auto_save, AutoSave::Off);
    assert_eq!(defaults.editor.auto_save_delay, 1000);

    for (name, want) in [
        ("off", AutoSave::Off),
        ("after_delay", AutoSave::AfterDelay),
        ("on_focus_change", AutoSave::OnFocusChange),
        ("on_window_change", AutoSave::OnWindowChange),
        ("on_insert_leave", AutoSave::OnInsertLeave),
    ] {
        let store = load(&format!("[editor]\nauto_save = \"{name}\"\n"));
        assert_eq!(store.diagnostics(), [], "{name}");
        assert_eq!(store.settings().editor.auto_save, want, "{name}");
    }

    let store = load("[editor]\nauto_save_delay = 250\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().editor.auto_save_delay, 250);

    for bad in ["auto_save = \"sometimes\"", "auto_save_delay = 50"] {
        let store = load(&format!("[editor]\n{bad}\n"));
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{bad}");
        assert_eq!(store.settings().editor.auto_save, AutoSave::Off, "{bad}");
        assert_eq!(store.settings().editor.auto_save_delay, 1000, "{bad}");
    }
}
