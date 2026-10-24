//! `[[vim.remap]]` with `mode = "i"` and `"c"`.

use fe_config::{SettingsStore, VimRemapMode};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn insert_and_cmdline_modes_parse() {
    let store = load(
        "[[vim.remap]]\nmode = \"i\"\nfrom = \"jk\"\nto = \"<Esc>\"\n\
         [[vim.remap]]\nmode = [\"c\", \"i\"]\nfrom = \"kj\"\nto = \"<CR>\"\n",
    );
    assert_eq!(store.diagnostics(), []);
    let remap = &store.settings().vim.remap;
    assert_eq!(remap[0].mode.modes(), [VimRemapMode::Insert]);
    assert_eq!(
        remap[1].mode.modes(),
        [VimRemapMode::CmdLine, VimRemapMode::Insert]
    );
}
