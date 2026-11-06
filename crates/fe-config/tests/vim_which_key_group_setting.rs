//! `[[vim.which_key_group]]`: names for groups of remaps in the which-key
//! popup.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, VimRemapMode};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn group_table_loads_and_validates() {
    assert!(Settings::default().vim.which_key_group.is_empty());

    let store = load(
        "[[vim.which_key_group]]\nkeys = \"<leader>f\"\nname = \"file/find\"\n\
         [[vim.which_key_group]]\nmode = \"x\"\nkeys = \"<leader>c\"\nname = \"code\"\n\
         [[vim.which_key_group]]\nmode = [\"n\", \"v\"]\nkeys = \"<leader>g\"\nname = \"git\"\n",
    );
    assert_eq!(store.diagnostics(), []);
    let groups = &store.settings().vim.which_key_group;
    assert_eq!(groups.len(), 3);
    // `mode` defaults to normal.
    assert_eq!(groups[0].mode.modes(), [VimRemapMode::Normal]);
    assert_eq!(
        (groups[0].keys.as_str(), groups[0].name.as_str()),
        ("<leader>f", "file/find")
    );
    assert_eq!(groups[1].mode.modes(), [VimRemapMode::Visual]);
    assert_eq!(
        groups[2].mode.modes(),
        [VimRemapMode::Normal, VimRemapMode::VisualSelect]
    );

    // Bad keys, a bad name and a bad mode are each rejected at their value,
    // and the table keeps its default.
    for (text, at) in [
        (
            "[[vim.which_key_group]]\nkeys = \"\"\nname = \"x\"\n",
            (2, 8),
        ),
        (
            "[[vim.which_key_group]]\nkeys = \"  \"\nname = \"x\"\n",
            (2, 8),
        ),
        ("[[vim.which_key_group]]\nkeys = 1\nname = \"x\"\n", (2, 8)),
        (
            "[[vim.which_key_group]]\nkeys = \"<leader>f\"\nname = \"\"\n",
            (3, 8),
        ),
        (
            "[[vim.which_key_group]]\nmode = \"s\"\nkeys = \"f\"\nname = \"x\"\n",
            (2, 8),
        ),
    ] {
        let store = load(text);
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{text}: {diagnostics:?}");
        assert_eq!((diagnostics[0].line, diagnostics[0].column), at, "{text}");
        assert!(store.settings().vim.which_key_group.is_empty(), "{text}");
    }
    // Keys and name are both needed.
    for text in [
        "[[vim.which_key_group]]\nname = \"x\"\n",
        "[[vim.which_key_group]]\nkeys = \"f\"\n",
    ] {
        let store = load(text);
        assert_eq!(store.diagnostics().len(), 1, "{text}");
        assert!(store.settings().vim.which_key_group.is_empty(), "{text}");
    }

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
