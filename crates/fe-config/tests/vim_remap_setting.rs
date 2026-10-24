//! `[[vim.remap]]` and `vim.leader`.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, VimRemapMode};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_remap_setting() {
    let defaults = Settings::default();
    assert_eq!(defaults.vim.leader, "\\");
    assert!(defaults.vim.remap.is_empty());

    let store = load(
        "[vim]\nleader = \"<Space>\"\n\
         [[vim.remap]]\nmode = \"n\"\nfrom = \"H\"\nto = \"^\"\n\
         [[vim.remap]]\nmode = [\"n\", \"v\", \"o\"]\nfrom = \"Y\"\nto = \"y$\"\nrecursive = true\n",
    );
    assert_eq!(store.diagnostics(), []);
    let vim = &store.settings().vim;
    assert_eq!(vim.leader, "<Space>");
    assert_eq!(vim.remap.len(), 2);
    assert_eq!(vim.remap[0].mode.modes(), [VimRemapMode::Normal]);
    assert!(!vim.remap[0].recursive);
    assert_eq!(
        vim.remap[1].mode.modes(),
        [
            VimRemapMode::Normal,
            VimRemapMode::VisualSelect,
            VimRemapMode::OperatorPending
        ]
    );
    assert!(vim.remap[1].recursive);

    let store = load("[[vim.remap]]\nmode = \"s\"\nfrom = \"a\"\nto = \"b\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    // Located at the `mode` value.
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (2, 8));
    assert!(store.settings().vim.remap.is_empty());

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
