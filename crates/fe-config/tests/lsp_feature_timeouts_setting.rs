//! `lsp.timeouts.code_action` and `lsp.timeouts.signature_help`.

use fe_config::{Settings, SettingsStore};

fn timeouts(settings: &Settings) -> serde_json::Value {
    serde_json::to_value(&settings.lsp.timeouts).unwrap()
}

#[test]
fn lsp_feature_timeouts_round_trip() {
    let defaults = timeouts(&Settings::default());
    assert_eq!(defaults["code_action"], 10000);
    assert_eq!(defaults["signature_help"], 3000);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(
        &user,
        "[lsp.timeouts]\ncode_action = 1500\nsignature_help = 700\n",
    )
    .unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    let loaded = timeouts(store.settings());
    assert_eq!(loaded["code_action"], 1500);
    assert_eq!(loaded["signature_help"], 700);
}
