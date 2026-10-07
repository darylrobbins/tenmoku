//! `write_setting`: one setting changed in a `settings.toml`, everything
//! else in it kept; and the `[status_bar]` table it writes to.

use std::{fs, path::Path};

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, write_setting};

/// The user settings at `path`.
fn load(path: &Path) -> SettingsStore {
    SettingsStore::load(Some(path.to_owned()), None)
}

/// The comment lines of `text`.
fn comments(text: &str) -> Vec<&str> {
    text.lines().filter(|line| line.starts_with('#')).collect()
}

/// The names in `dir`.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn status_bar_setting() {
    assert!(Settings::default().status_bar.visible);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    fs::write(&file, "[status_bar]\nvisible = false\n").unwrap();
    let store = load(&file);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().status_bar.visible);
}

#[test]
fn preserves_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    let original = "# My settings.\n\
                    \n\
                    [editor]\n\
                    font_size = 15.0   # bigger\n\
                    \n\
                    \n\
                    [vim]\n\
                    enabled = false # mine\n\
                    # The end.\n";
    fs::write(&file, original).unwrap();

    write_setting(&file, &["vim", "enabled"], true).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    assert_eq!(
        written,
        original.replace("enabled = false # mine", "enabled = true # mine")
    );

    write_setting(&file, &["status_bar", "visible"], false).unwrap();
    let more = fs::read_to_string(&file).unwrap();
    let added = more
        .strip_prefix(&written)
        .unwrap_or_else(|| panic!("the text before is kept: {more:?}"));
    assert_eq!(added.trim(), "[status_bar]\nvisible = false", "{more:?}");

    let store = load(&file);
    assert_eq!(store.diagnostics(), []);
    let settings = store.settings();
    assert!(settings.vim.enabled);
    assert!(!settings.status_bar.visible);
    assert_eq!(settings.editor.font_size, 15.0);
    assert_eq!(names(dir.path()), ["settings.toml"]);
}

#[test]
fn creates_from_the_template() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir
        .path()
        .join("config")
        .join("fastedit")
        .join("settings.toml");
    write_setting(&file, &["vim", "enabled"], true).unwrap();

    let store = load(&file);
    assert_eq!(store.diagnostics(), []);
    let mut expected = Settings::default();
    expected.vim.enabled = true;
    assert_eq!(**store.settings(), expected);
    let written = fs::read_to_string(&file).unwrap();
    let comments = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(comments(&written), comments(USER_SETTINGS_TEMPLATE));

    // Through a symlink: the target changes, and the link stays a link.
    let link = dir.path().join("settings.toml");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    write_setting(&link, &["status_bar", "visible"], false).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&link).unwrap(), file);
    let settings = load(&file).settings().clone();
    assert!(settings.vim.enabled);
    assert!(!settings.status_bar.visible);
    assert_eq!(names(file.parent().unwrap()), ["settings.toml"]);
}

#[test]
fn refuses_bad_files() {
    // The template cut short is no exception.
    let cut = USER_SETTINGS_TEMPLATE.find("[vim").unwrap() + "[vim".len();
    let cut = &USER_SETTINGS_TEMPLATE[..cut];
    for bad in ["[vim\nenabled = false\n", "vim = 3\n", cut] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.toml");
        fs::write(&file, bad).unwrap();
        let result = write_setting(&file, &["vim", "enabled"], true);
        assert!(result.is_err(), "{bad:?} is refused");
        assert_eq!(fs::read_to_string(&file).unwrap(), bad);
        assert_eq!(names(dir.path()), ["settings.toml"]);
    }
}

#[test]
fn keeps_line_endings() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    for (original, after) in [
        // CRLF throughout, comments included; what's added uses it too.
        (
            "# Mine.\r\n[editor]\r\nfont_size = 15.0 # big\r\n\r\n[vim]\r\nenabled = false\r\n",
            "# Mine.\r\n[editor]\r\nfont_size = 15.0 # big\r\n\r\n[vim]\r\nenabled = true\r\n",
        ),
        // No final newline.
        (
            "[vim]\nenabled = false # mine",
            "[vim]\nenabled = true # mine",
        ),
        // A byte order mark.
        (
            "\u{feff}[vim]\r\nenabled = false\r\n# c\r\n",
            "\u{feff}[vim]\r\nenabled = true\r\n# c\r\n",
        ),
    ] {
        fs::write(&file, original).unwrap();
        write_setting(&file, &["vim", "enabled"], true).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), after);
    }

    fs::write(&file, "[vim]\r\nenabled = true\r\n").unwrap();
    write_setting(&file, &["status_bar", "visible"], false).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[vim]\r\nenabled = true\r\n\r\n[status_bar]\r\nvisible = false\r\n"
    );
    fs::write(&file, "[editor]\r\ntab_size = 2\r\n").unwrap();
    write_setting(&file, &["editor", "soft_wrap"], true).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[editor]\r\ntab_size = 2\r\nsoft_wrap = true\r\n"
    );
}

/// toml_edit would write some files back with more changed than a key it
/// adds: it gathers a dotted table's keys, and respaces a dotted header
/// whose table is defined again later. An existing key is still changed in
/// place; adding one is refused, leaving the file untouched.
#[test]
fn refuses_to_reformat() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    let original = "vim.x = 1\neditor.tab_size = 2\nvim.enabled = false # mine\n";
    fs::write(&file, original).unwrap();
    write_setting(&file, &["vim", "enabled"], true).unwrap();
    let changed = original.replace("false", "true");
    assert_eq!(fs::read_to_string(&file).unwrap(), changed);

    for original in [
        "[vim]\nx.a = 1\nother = 2\nx.b = 3\n",
        "vim.x = 1\neditor.tab_size = 2\nvim.y = 2\n",
        "[vim . sub]\nx = 1\n[vim]\nother = 2\n",
    ] {
        fs::write(&file, original).unwrap();
        let result = write_setting(&file, &["vim", "enabled"], true);
        assert!(result.is_err(), "{original:?} is refused");
        assert_eq!(fs::read_to_string(&file).unwrap(), original);
        assert_eq!(names(dir.path()), ["settings.toml"]);
    }
}

/// An empty file has nothing to keep, and gets the template; any other
/// file is edited as it is, even the start of the template.
#[test]
fn only_an_empty_file_gets_the_template() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    // A byte order mark aside.
    for empty in ["", "\u{feff}"] {
        fs::write(&file, empty).unwrap();
        write_setting(&file, &["vim", "enabled"], true).unwrap();
        let written = fs::read_to_string(&file).unwrap();
        assert!(written.starts_with(empty), "{written:?}");
        assert_eq!(
            comments(&written[empty.len()..]),
            comments(USER_SETTINGS_TEMPLATE)
        );
        assert!(load(&file).settings().vim.enabled);
    }

    let cut = &USER_SETTINGS_TEMPLATE[..USER_SETTINGS_TEMPLATE.find("[vim]").unwrap()];
    fs::write(&file, cut).unwrap();
    write_setting(&file, &["vim", "enabled"], true).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    let added = written.strip_prefix(cut).map(str::trim);
    assert_eq!(added, Some("[vim]\nenabled = true"), "{written:?}");
}

/// With no final newline, what's added after the file's end still starts
/// on a line of its own.
#[test]
fn adds_after_a_last_line_with_no_newline() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    for (original, after) in [
        ("[vim]", "[vim]\nenabled = true"),
        ("[vim] # mine", "[vim] # mine\nenabled = true"),
        ("[vim]\nother = 1", "[vim]\nother = 1\nenabled = true"),
        ("[vim]\r\nother = 1", "[vim]\r\nother = 1\r\nenabled = true"),
    ] {
        fs::write(&file, original).unwrap();
        write_setting(&file, &["vim", "enabled"], true).unwrap();
        let written = fs::read_to_string(&file).unwrap();
        assert!(
            written.starts_with(after),
            "{original:?} became {written:?}"
        );
        assert!(written.parse::<toml::Table>().is_ok(), "{written:?}");
        assert!(load(&file).settings().vim.enabled, "{written:?}");

        write_setting(&file, &["status_bar", "visible"], false).unwrap();
        let written = fs::read_to_string(&file).unwrap();
        let store = load(&file);
        // `vim.other` is unknown: a warning.
        assert!(
            store.diagnostics().iter().all(|d| !d.is_error()),
            "{written:?}"
        );
        assert!(store.settings().vim.enabled, "{written:?}");
        assert!(!store.settings().status_bar.visible, "{written:?}");
    }
}
