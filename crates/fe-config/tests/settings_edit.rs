//! The Settings editor's writes: one setting set in, or reset by removing
//! it from, the user's `settings.toml` or the project's, keeping the rest of
//! the file.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use fe_config::{
    SettingEditError, SettingScope, SettingValue, USER_SETTINGS_TEMPLATE, reset_setting,
    set_setting,
};

static SERIAL: Mutex<()> = Mutex::new(());

/// A config directory and a project root of the test's own, with no other
/// test of this binary running while the environment names them.
struct Dirs {
    _guard: MutexGuard<'static, ()>,
    _temp: tempfile::TempDir,
    config: PathBuf,
    root: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let guard = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        // SAFETY: every test takes the lock first, so no other thread is
        // running to read the environment.
        unsafe { std::env::set_var("FASTEDIT_CONFIG_DIR", &config) };
        Self {
            _guard: guard,
            _temp: temp,
            config,
            root,
        }
    }

    fn user(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    fn workspace(&self) -> PathBuf {
        self.root.join(".fastedit/settings.toml")
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn modified(path: &Path) -> std::time::SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn set_in_user_file() {
    let dirs = Dirs::new();
    assert_eq!(fe_config::user_settings_path(), Some(dirs.user()));

    // A missing file starts from the template; its commented default
    // becomes the setting.
    set_setting(
        SettingScope::User,
        &dirs.root,
        &["editor", "font_size"],
        SettingValue::Float(16.0),
    )
    .unwrap();
    let expected = USER_SETTINGS_TEMPLATE.replacen("# font_size = 13.0", "font_size = 16.0", 1);
    assert_eq!(read(&dirs.user()), expected);
    assert!(!dirs.workspace().exists());

    for (key, value) in [
        (&["editor", "tab_size"][..], SettingValue::Int(2)),
        (&["editor", "soft_wrap"], SettingValue::Bool(true)),
        (&["editor", "auto_save"], SettingValue::Str("after_delay")),
        (
            &["editor", "code_actions_on_save"],
            SettingValue::StrList(&["source.organizeImports"]),
        ),
    ] {
        set_setting(SettingScope::User, &dirs.root, key, value).unwrap();
    }
    let store = fe_config::SettingsStore::load(Some(dirs.user()), None);
    let editor = &store.settings().editor;
    assert_eq!(editor.font_size, 16.0);
    assert_eq!(editor.tab_size, 2);
    assert!(editor.soft_wrap);
    assert_eq!(editor.code_actions_on_save, ["source.organizeImports"]);
    assert!(store.diagnostics().is_empty(), "{:?}", store.diagnostics());
}

#[test]
fn set_in_workspace_file_creates_it_empty() {
    let dirs = Dirs::new();
    set_setting(
        SettingScope::Workspace,
        &dirs.root,
        &["editor", "tab_size"],
        SettingValue::Int(2),
    )
    .unwrap();
    assert_eq!(
        fe_config::project_settings_path(&dirs.root),
        dirs.workspace()
    );
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 2\n");
    assert!(!dirs.user().exists());

    // A top-level table's key next.
    set_setting(
        SettingScope::Workspace,
        &dirs.root,
        &["vim", "enabled"],
        SettingValue::Bool(true),
    )
    .unwrap();
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ntab_size = 2\n\n[vim]\nenabled = true\n"
    );
}

#[test]
fn reset_removes_key_and_empty_table() {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.workspace().parent().unwrap()).unwrap();
    fs::write(
        dirs.workspace(),
        "[vim]\nenabled = true\n\n[editor]\ntab_size = 2\n",
    )
    .unwrap();
    reset_setting(SettingScope::Workspace, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert_eq!(read(&dirs.workspace()), "[vim]\nenabled = true\n");

    // A nested table, and the table over it.
    fs::write(
        dirs.workspace(),
        "[editor.minimap]\nenabled = false\n\n[vim]\nenabled = true\n",
    )
    .unwrap();
    reset_setting(
        SettingScope::Workspace,
        &dirs.root,
        &["editor", "minimap", "enabled"],
    )
    .unwrap();
    assert_eq!(read(&dirs.workspace()), "[vim]\nenabled = true\n");

    // In the user's file too.
    fs::create_dir_all(&dirs.config).unwrap();
    fs::write(dirs.user(), "[editor]\ntab_size = 2\n").unwrap();
    reset_setting(SettingScope::User, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert_eq!(read(&dirs.user()), "");
}

#[test]
fn reset_keeps_a_table_with_other_keys() {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.workspace().parent().unwrap()).unwrap();
    fs::write(
        dirs.workspace(),
        "[editor]\ntab_size = 2\nsoft_wrap = true\n",
    )
    .unwrap();
    reset_setting(SettingScope::Workspace, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = true\n");

    // Or one with a table of its own in it.
    fs::write(
        dirs.workspace(),
        "[editor]\ntab_size = 2\n\n[editor.minimap]\nenabled = false\n",
    )
    .unwrap();
    reset_setting(SettingScope::Workspace, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\n\n[editor.minimap]\nenabled = false\n"
    );
}

#[test]
fn comments_survive_set_and_reset() {
    let dirs = Dirs::new();
    fs::create_dir_all(&dirs.config).unwrap();
    let text = "# Mine.\n\n[editor]\n# Small.\ntab_size = 2 # spaces\n\n\
                # Wrap.\nsoft_wrap = true\n";
    fs::write(dirs.user(), text).unwrap();

    set_setting(
        SettingScope::User,
        &dirs.root,
        &["editor", "tab_size"],
        SettingValue::Int(8),
    )
    .unwrap();
    assert_eq!(read(&dirs.user()), text.replace("= 2", "= 8"));

    reset_setting(SettingScope::User, &dirs.root, &["editor", "soft_wrap"]).unwrap();
    assert_eq!(
        read(&dirs.user()),
        "# Mine.\n\n[editor]\n# Small.\ntab_size = 8 # spaces\n\n# Wrap.\n"
    );

    // A table left with only a comment stays, the comment in it.
    reset_setting(SettingScope::User, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert_eq!(
        read(&dirs.user()),
        "# Mine.\n\n[editor]\n# Small.\n\n# Wrap.\n"
    );

    // So does an inline table with a comment after it or inside it.
    for (text, expected) in [
        (
            "editor = { tab_size = 2 } # keep editor\n",
            "editor = {} # keep editor\n",
        ),
        (
            "editor = {\n  # keep editor\n  tab_size = 2,\n}\n",
            "editor = {\n  # keep editor\n}\n",
        ),
        (
            "editor = {\n  # keep editor\n  tab_size = 2 }\n",
            "editor = {\n  # keep editor\n   }\n",
        ),
        (
            "editor = { tab_size = 2, font_family = \"F# Code\" }\n",
            "editor = { font_family = \"F# Code\" }\n",
        ),
    ] {
        fs::write(dirs.user(), text).unwrap();
        reset_setting(SettingScope::User, &dirs.root, &["editor", "tab_size"]).unwrap();
        assert_eq!(read(&dirs.user()), expected);
    }

    // The template's commented defaults stay through a set and a reset.
    fs::remove_file(dirs.user()).unwrap();
    let key = &["editor", "minimap", "max_column"];
    set_setting(SettingScope::User, &dirs.root, key, SettingValue::Int(80)).unwrap();
    reset_setting(SettingScope::User, &dirs.root, key).unwrap();
    let written = read(&dirs.user());
    let template: Vec<&str> = USER_SETTINGS_TEMPLATE.lines().collect();
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines.len(), template.len() - 1, "{written}");
    assert!(
        template.iter().filter(|line| !lines.contains(line)).count() == 1,
        "only the commented default became the setting and went: {written}"
    );
}

#[test]
fn reset_of_an_absent_key_writes_nothing() {
    let dirs = Dirs::new();
    // Missing files stay missing.
    reset_setting(SettingScope::User, &dirs.root, &["editor", "tab_size"]).unwrap();
    reset_setting(SettingScope::Workspace, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert!(!dirs.config.exists());
    assert!(!dirs.root.join(".fastedit").exists());

    // A file without the key isn't written, an empty table in it kept.
    fs::create_dir_all(dirs.workspace().parent().unwrap()).unwrap();
    fs::write(dirs.workspace(), "[editor]\n").unwrap();
    let before = modified(&dirs.workspace());
    std::thread::sleep(Duration::from_millis(20));
    reset_setting(SettingScope::Workspace, &dirs.root, &["editor", "tab_size"]).unwrap();
    assert_eq!(modified(&dirs.workspace()), before);
    assert_eq!(read(&dirs.workspace()), "[editor]\n");
}

#[test]
fn workspace_restricted_keys_are_refused() {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.workspace().parent().unwrap()).unwrap();
    let text = "# Mine.\n[terminal]\nshell = \"/bin/sh\"\n";
    fs::write(dirs.workspace(), text).unwrap();

    // PROJECT_CHAT, PROJECT_TERMINAL_SHELL and PROJECT_VIM_USER_ONLY's keys.
    let refused: [(&[&str], SettingValue); 5] = [
        (&["chat", "model"], SettingValue::Str("claude-opus-5-5")),
        (&["chat", "show_thinking"], SettingValue::Bool(true)),
        (&["terminal", "shell"], SettingValue::Str("/bin/zsh")),
        (&["terminal", "shell_args"], SettingValue::StrList(&["-l"])),
        (&["vim", "shell"], SettingValue::Str("/bin/bash")),
    ];
    for (key, value) in refused {
        let set = set_setting(SettingScope::Workspace, &dirs.root, key, value);
        assert!(
            matches!(set, Err(SettingEditError::NotAllowedInWorkspace)),
            "{key:?}: {set:?}"
        );
        let reset = reset_setting(SettingScope::Workspace, &dirs.root, key);
        assert!(
            matches!(reset, Err(SettingEditError::NotAllowedInWorkspace)),
            "{key:?}: {reset:?}"
        );
        assert_eq!(read(&dirs.workspace()), text, "{key:?}");
    }
    let remap = reset_setting(SettingScope::Workspace, &dirs.root, &["vim", "remap"]);
    assert!(matches!(
        remap,
        Err(SettingEditError::NotAllowedInWorkspace)
    ));
    assert_eq!(read(&dirs.workspace()), text);

    // The user's file takes them, and the project's the keys beside them.
    for (key, value) in refused {
        set_setting(SettingScope::User, &dirs.root, key, value).unwrap();
    }
    set_setting(
        SettingScope::Workspace,
        &dirs.root,
        &["terminal", "font_size"],
        SettingValue::Float(12.0),
    )
    .unwrap();
    assert_eq!(read(&dirs.workspace()), format!("{text}font_size = 12.0\n"));
}

#[test]
fn out_of_range_is_refused() {
    let dirs = Dirs::new();
    let refused: [(&[&str], SettingValue); 5] = [
        (&["editor", "tab_size"], SettingValue::Int(17)),
        (&["editor", "tab_size"], SettingValue::Int(0)),
        (&["editor", "font_size"], SettingValue::Float(72.5)),
        (&["chat", "max_tokens"], SettingValue::Int(10)),
        (&["chat", "model"], SettingValue::Str("")),
    ];
    for (key, value) in refused {
        let set = set_setting(SettingScope::User, &dirs.root, key, value);
        assert!(
            matches!(&set, Err(SettingEditError::Invalid(message)) if message.contains(&key.join("."))),
            "{key:?}: {set:?}"
        );
    }
    // A float that isn't finite.
    let set = set_setting(
        SettingScope::User,
        &dirs.root,
        &["editor", "font_size"],
        SettingValue::Float(f64::NAN),
    );
    assert!(matches!(set, Err(SettingEditError::Invalid(_))), "{set:?}");

    // And of the wrong type.
    let wrong: [(&[&str], SettingValue); 4] = [
        (&["editor", "soft_wrap"], SettingValue::Str("yes")),
        (&["editor", "tab_size"], SettingValue::Float(2.5)),
        (&["editor", "font_size"], SettingValue::Bool(true)),
        (&["editor", "code_actions_on_save"], SettingValue::Str("x")),
    ];
    for (key, value) in wrong {
        let set = set_setting(SettingScope::User, &dirs.root, key, value);
        assert!(
            matches!(set, Err(SettingEditError::Invalid(_))),
            "{key:?}: {set:?}"
        );
    }
    assert!(!dirs.config.exists(), "nothing written");

    // The bounds themselves are fine.
    set_setting(
        SettingScope::User,
        &dirs.root,
        &["editor", "tab_size"],
        SettingValue::Int(16),
    )
    .unwrap();
    set_setting(
        SettingScope::User,
        &dirs.root,
        &["editor", "font_size"],
        SettingValue::Int(72),
    )
    .unwrap();
}

#[test]
fn unknown_enum_variant_is_refused() {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.workspace().parent().unwrap()).unwrap();
    fs::write(dirs.workspace(), "[editor]\nauto_save = \"off\"\n").unwrap();

    for (key, value) in [
        (&["editor", "auto_save"][..], "sometimes"),
        (&["editor", "minimap", "side"], "top"),
        (&["editor", "auto_save"], "Off"),
    ] {
        let set = set_setting(
            SettingScope::Workspace,
            &dirs.root,
            key,
            SettingValue::Str(value),
        );
        assert!(
            matches!(set, Err(SettingEditError::Invalid(_))),
            "{key:?} = {value}: {set:?}"
        );
    }
    assert_eq!(read(&dirs.workspace()), "[editor]\nauto_save = \"off\"\n");

    set_setting(
        SettingScope::Workspace,
        &dirs.root,
        &["editor", "auto_save"],
        SettingValue::Str("on_focus_change"),
    )
    .unwrap();
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\nauto_save = \"on_focus_change\"\n"
    );
}

#[test]
fn reset_of_a_dotted_key_in_a_commented_inline_table_keeps_every_comment() {
    let dirs = Dirs::new();
    fs::create_dir_all(&dirs.config).unwrap();
    for (text, want) in [
        (
            "editor = {\n  # keep editor\n  minimap.enabled = true }\n",
            "editor = {\n  # keep editor\n   }\n",
        ),
        (
            "editor = { tab_size = 2,\n  # keep minimap\n  minimap.enabled = true }\n",
            "editor = { tab_size = 2\n  # keep minimap\n   }\n",
        ),
    ] {
        fs::write(dirs.user(), text).unwrap();
        reset_setting(
            SettingScope::User,
            &dirs.root,
            &["editor", "minimap", "enabled"],
        )
        .unwrap();
        assert_eq!(read(&dirs.user()), want, "{text}");
    }

    // On a line of its own it goes with its line, the comment kept.
    fs::write(
        dirs.user(),
        "editor = {\n  # keep editor\n  minimap.enabled = true,\n  tab_size = 2 }\n",
    )
    .unwrap();
    reset_setting(
        SettingScope::User,
        &dirs.root,
        &["editor", "minimap", "enabled"],
    )
    .unwrap();
    assert_eq!(
        read(&dirs.user()),
        "editor = {\n  # keep editor\n  tab_size = 2 }\n"
    );
}

#[test]
fn reset_of_a_dotted_key_without_comments_still_works() {
    let dirs = Dirs::new();
    fs::create_dir_all(&dirs.config).unwrap();
    let key = &["editor", "minimap", "enabled"];

    fs::write(
        dirs.user(),
        "# Mine.\neditor = { minimap.enabled = true, tab_size = 2 }\n",
    )
    .unwrap();
    reset_setting(SettingScope::User, &dirs.root, key).unwrap();
    let written = read(&dirs.user());
    assert!(written.starts_with("# Mine.\n"), "{written}");
    let parsed: toml::Table = written.parse().unwrap();
    assert_eq!(
        parsed["editor"]
            .as_table()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["tab_size"],
        "{written}"
    );

    // Its table emptied goes too.
    fs::write(dirs.user(), "a = 1\neditor = { minimap.enabled = true }\n").unwrap();
    reset_setting(SettingScope::User, &dirs.root, key).unwrap();
    assert_eq!(read(&dirs.user()), "a = 1\n");
}
