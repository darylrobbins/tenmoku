//! `vim.jump_style`: `"leap"` by default, or `"flash"`.

use std::path::PathBuf;

use fe_config::{
    Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, VimJumpStyle, settings_schema,
};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_jump_style_setting() {
    assert_eq!(Settings::default().vim.jump_style, VimJumpStyle::Leap);

    let store = load("[vim]\njump_style = \"leap\"\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.jump_style, VimJumpStyle::Leap);

    let store = load("[vim]\njump_style = \"flash\"\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.jump_style, VimJumpStyle::Flash);

    let store = load("[vim]\njump_on_s = true\njump_style = \"hop\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    // Located at the `jump_style` value.
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (3, 14));
    assert_eq!(store.settings().vim.jump_style, VimJumpStyle::Leap);

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# jump_style = \"leap\"\n"));
    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    // The committed schema is the settings' (read at run time: a shared
    // target can hold a test binary built for another checkout), and lists
    // both values with the default.
    let path =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("settings.schema.json");
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let schema = settings_schema();
    assert_eq!(
        golden, schema,
        "settings.schema.json is out of date: rerun schema_matches_golden with UPDATE_GOLDEN=1"
    );
    let property = &schema["$defs"]["VimSettings"]["properties"]["jump_style"];
    assert_eq!(property["default"], "leap");
    let values = schema["$defs"]["VimJumpStyle"].to_string();
    for value in ["leap", "flash"] {
        assert!(
            values.contains(&format!("\"{value}\"")),
            "{value}: {values}"
        );
    }
}
