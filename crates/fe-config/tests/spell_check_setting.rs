//! `editor.spell_check`, `editor.spell_check_scopes` and
//! `editor.spell_check_language`.

use std::path::PathBuf;

use fe_config::{
    Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE,
    settings::{EditorSettings, LspLayer, SpellScope},
    settings_schema,
};

const ALL: [SpellScope; 3] = [SpellScope::Comments, SpellScope::Strings, SpellScope::Text];

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
fn spell_check_setting() {
    let defaults = Settings::default().editor;
    assert!(defaults.spell_check);
    assert_eq!(defaults.spell_check_scopes, ALL);
    assert_eq!(defaults.spell_check_language, "auto");

    let (store, _dir) = load(
        "[editor]\n\
         spell_check = false\n\
         spell_check_scopes = [\"text\", \"comments\"]\n\
         spell_check_language = \"en-GB\"\n",
    );
    assert_eq!(store.diagnostics(), []);
    assert!(!editor(&store).spell_check);
    assert_eq!(
        editor(&store).spell_check_scopes,
        [SpellScope::Text, SpellScope::Comments]
    );
    assert_eq!(editor(&store).spell_check_language, "en-GB");

    // No scopes is a choice, not a mistake.
    let (store, _dir) = load("[editor]\nspell_check_scopes = []\n");
    assert_eq!(store.diagnostics(), []);
    assert!(editor(&store).spell_check_scopes.is_empty());

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# spell_check = true\n"));
    assert!(
        USER_SETTINGS_TEMPLATE
            .contains("\n# spell_check_scopes = [\"comments\", \"strings\", \"text\"]\n")
    );
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# spell_check_language = \"auto\"\n"));
    let (store, _dir) = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    let path =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("settings.schema.json");
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let schema = settings_schema();
    assert_eq!(
        golden, schema,
        "settings.schema.json is out of date: rerun schema_matches_golden with UPDATE_GOLDEN=1"
    );
    let properties = &schema["$defs"]["EditorSettings"]["properties"];
    assert_eq!(properties["spell_check"]["default"], true);
    assert_eq!(
        properties["spell_check_scopes"]["default"],
        serde_json::json!(["comments", "strings", "text"])
    );
    assert_eq!(properties["spell_check_language"]["default"], "auto");
}

#[test]
fn a_bad_scope_is_a_warning_that_keeps_the_valid_ones() {
    let (store, _dir) = load(
        "[editor]\n\
         tab_size = 2\n\
         spell_check_scopes = [\n  \"comments\",\n  \"code\",\n  \"text\",\n  7,\n  \"Strings\",\n]\n",
    );
    assert_eq!(
        editor(&store).spell_check_scopes,
        [SpellScope::Comments, SpellScope::Text]
    );
    // The rest of the file still applies.
    assert_eq!(editor(&store).tab_size, 2);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line))
        .collect();
    let key = Some("editor.spell_check_scopes");
    assert_eq!(
        found,
        [
            (Severity::Warning, key, 5),
            (Severity::Warning, key, 7),
            (Severity::Warning, key, 8),
        ],
        "each warning is at its entry"
    );
    assert!(
        store.diagnostics()[0].message.contains("\"code\""),
        "{}",
        store.diagnostics()[0].message
    );
    assert!(
        store.diagnostics()[0].message.contains("\"comments\""),
        "it says what would do: {}",
        store.diagnostics()[0].message
    );
}

#[test]
fn every_way_to_write_the_key_is_filtered() {
    for text in [
        "editor.spell_check_scopes = [\"text\", \"nope\"]\n",
        "editor = { spell_check_scopes = [\"text\", \"nope\"] }\n",
    ] {
        let (store, _dir) = load(text);
        assert_eq!(
            editor(&store).spell_check_scopes,
            [SpellScope::Text],
            "{text}"
        );
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{text}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Warning, "{text}");
        assert!(diagnostics[0].message.contains("\"nope\""), "{text}");
    }
}

#[test]
fn only_bad_scopes_leave_none() {
    let (store, _dir) = load("[editor]\nspell_check_scopes = [\"nope\"]\n");
    assert!(editor(&store).spell_check_scopes.is_empty());
    assert_eq!(store.diagnostics().len(), 1);
}

#[test]
fn a_value_that_is_not_a_list_keeps_the_default() {
    for bad in ["\"text\"", "true", "1", "{ text = true }"] {
        let (store, _dir) = load(&format!(
            "[editor]\ntab_size = 2\nspell_check_scopes = {bad}\n"
        ));
        assert_eq!(editor(&store).spell_check_scopes, ALL, "{bad}");
        assert_eq!(editor(&store).tab_size, 2, "{bad}");
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Warning, "{bad}");
        assert_eq!(
            diagnostics[0].key.as_deref(),
            Some("editor.spell_check_scopes")
        );
        assert_eq!(diagnostics[0].line, 3, "{bad}");
    }
}

#[test]
fn other_editor_errors_keep_their_key_and_line() {
    // The filter governs the one key, so an error in another still has its
    // place in the file.
    let (store, _dir) = load("[editor]\n\ntab_size = \"wide\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].key.as_deref(), Some("editor.tab_size"));
    assert_eq!(diagnostics[0].line, 3);
}

#[test]
fn an_empty_language_is_an_error() {
    let (store, _dir) = load("[editor]\nspell_check_language = \"\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(editor(&store).spell_check_language, "auto");
}

#[test]
fn a_project_file_filters_too() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.toml");
    std::fs::write(
        &project,
        "[editor]\nspell_check_scopes = [\"nope\", \"text\"]\n",
    )
    .unwrap();
    let store = SettingsStore::load(None, Some(project));
    assert_eq!(editor(&store).spell_check_scopes, [SpellScope::Text]);
    assert_eq!(store.diagnostics().len(), 1);
}

#[test]
fn a_bad_scope_does_not_cost_a_trusted_root_its_lsp_table() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(
        &user,
        "[editor]\nspell_check_scopes = [\"nope\"]\n\n[lsp]\nenabled = false\n",
    )
    .unwrap();
    let mut layer = LspLayer::user(user);
    layer.read();
    assert!(!layer.apply(&Settings::default().lsp).enabled);
}
