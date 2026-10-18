//! `editor.inlay_hints`, `semantic_highlighting`, `lightbulb`, `code_lens` and
//! `code_actions_on_save`.

use fe_config::SettingsStore;
use fe_config::settings::EditorSettings;

fn load(body: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, body).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn lsp_editor_settings_round_trip() {
    let d = EditorSettings::default();
    assert!(d.inlay_hints && d.semantic_highlighting && d.lightbulb && d.code_lens);
    assert!(d.code_actions_on_save.is_empty());

    let store = load(
        "[editor]\ninlay_hints = false\nsemantic_highlighting = false\nlightbulb = false\n\
         code_lens = false\ncode_actions_on_save = [\"source.organizeImports\"]\n",
    );
    assert_eq!(store.diagnostics(), []);
    let e = &store.settings().editor;
    assert!(!e.inlay_hints && !e.semantic_highlighting && !e.lightbulb && !e.code_lens);
    assert_eq!(e.code_actions_on_save, ["source.organizeImports"]);

    let store = load("[editor]\ncode_actions_on_save = \"source.fixAll\"\n");
    assert!(!store.diagnostics().is_empty());
    assert!(store.settings().editor.code_actions_on_save.is_empty());
}
