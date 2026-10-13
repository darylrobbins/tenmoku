//! The `terminal` settings: defaults, ranges, and a project file unable to
//! choose the shell.

use fe_config::{PROJECT_TERMINAL_SHELL, Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

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
fn defaults() {
    let terminal = Settings::default().terminal;
    assert_eq!(terminal.shell, "");
    assert!(terminal.shell_args.is_empty());
    assert_eq!(terminal.scrollback, 10_000);
    assert_eq!(terminal.font_family, None);
    assert_eq!(terminal.font_size, None);
    let store = load(USER_SETTINGS_TEMPLATE, None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}

#[test]
fn a_user_file_sets_each_key() {
    let store = load(
        "[terminal]\nshell = \"/bin/fish\"\nshell_args = [\"-l\", \"-i\"]\nscrollback = 500\nfont_family = \"Menlo\"\nfont_size = 11.5\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let terminal = &store.settings().terminal;
    assert_eq!(terminal.shell, "/bin/fish");
    assert_eq!(terminal.shell_args, ["-l", "-i"]);
    assert_eq!(terminal.scrollback, 500);
    assert_eq!(terminal.font_family.as_deref(), Some("Menlo"));
    assert_eq!(terminal.font_size, Some(11.5));
}

#[test]
fn out_of_range_values_keep_the_last_good_one() {
    for source in [
        "[terminal]\nscrollback = 100001\n",
        "[terminal]\nfont_size = 5.0\n",
        "[terminal]\nfont_size = 73.0\n",
    ] {
        let store = load(source, None);
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
        assert_eq!(
            store.settings().terminal,
            Settings::default().terminal,
            "{source}"
        );
    }
}

#[test]
fn a_project_file_cannot_set_the_shell() {
    let store = load(
        "",
        Some(
            "[terminal]\nshell = \"/bin/evil\"\nshell_args = [\"-c\", \"x\"]\nscrollback = 42\nfont_size = 20.0\n",
        ),
    );
    let terminal = &store.settings().terminal;
    assert_eq!(terminal.shell, "");
    assert!(terminal.shell_args.is_empty());
    assert_eq!(terminal.scrollback, 42);
    assert_eq!(terminal.font_size, Some(20.0));
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    let mut keys: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            assert!(!d.is_error(), "{d:?}");
            assert_eq!(d.message, PROJECT_TERMINAL_SHELL);
            d.key.clone().unwrap()
        })
        .collect();
    keys.sort();
    assert_eq!(keys, ["terminal.shell", "terminal.shell_args"]);
}
