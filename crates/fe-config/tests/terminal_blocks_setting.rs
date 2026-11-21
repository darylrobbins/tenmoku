//! `terminal.command_blocks` (on): the edge and label of each command block.

use fe_config::{Settings, SettingsStore};

#[test]
fn terminal_blocks_defaults_true() {
    assert!(Settings::default().terminal.command_blocks);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, "[terminal]\ncommand_blocks = false\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().terminal.command_blocks);
}
