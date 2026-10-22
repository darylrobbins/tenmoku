//! `vim.shell` and `vim.filter_time_limit`, and a project file unable to set
//! `vim.shell` or `vim.remap`.

use fe_config::{PROJECT_VIM_USER_ONLY, Settings, SettingsStore};

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|contents| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, contents).unwrap();
        path
    });
    SettingsStore::load(Some(user_path), project_path)
}

#[test]
fn vim_filter_settings_and_project_files() {
    let defaults = Settings::default().vim;
    assert_eq!(defaults.shell, "");
    assert_eq!(defaults.filter_time_limit, 10_000);

    let store = load(
        "[vim]\nshell = \"/bin/zsh\"\nfilter_time_limit = 250\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.shell, "/bin/zsh");
    assert_eq!(store.settings().vim.filter_time_limit, 250);

    let store = load("[vim]\nfilter_time_limit = 99\n", None);
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
    assert_eq!(diagnostics[0].key.as_deref(), Some("vim.filter_time_limit"));
    assert_eq!(store.settings().vim.filter_time_limit, 10_000);

    let store = load(
        "",
        Some(
            "[vim]\nshell = \"/bin/evil\"\njump_on_s = true\n\n[[vim.remap]]\nmode = \"n\"\nfrom = \"x\"\nto = \":%!evil<CR>\"\n",
        ),
    );
    let vim = &store.settings().vim;
    assert_eq!(vim.shell, "");
    assert!(vim.remap.is_empty());
    assert!(vim.jump_on_s);
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    let mut keys: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            assert!(!d.is_error(), "{d:?}");
            assert_eq!(d.message, PROJECT_VIM_USER_ONLY);
            d.key.clone().unwrap()
        })
        .collect();
    keys.sort();
    assert_eq!(keys, ["vim.remap", "vim.shell"]);
}
