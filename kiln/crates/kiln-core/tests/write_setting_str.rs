// SPDX-License-Identifier: Apache-2.0

//! Writing a string setting, and several changes at once, into a
//! `settings.toml`, keeping the rest of it.

use std::fs;

use kiln_core::write::{SettingValue, write_setting_str, write_settings};

/// The commented-out defaults a missing file is created from.
const TEMPLATE: &str = "# Settings.\n\n[theme]\n# The theme, by name.\n# name = \"Dark\"\n";

/// `text`'s string at `table.key`.
fn string(text: &str, table: &str, key: &str) -> Option<String> {
    let parsed: toml::Table = text.parse().unwrap();
    let table = parsed.get(table)?.as_table()?;
    Some(table.get(key)?.as_str()?.to_owned())
}

#[test]
fn sets_and_inserts_strings() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config/settings.toml");

    // A missing file starts from the template, the default's comment made
    // the setting.
    write_setting_str(&file, &["theme", "name"], "Light", TEMPLATE).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Settings.\n\n[theme]\n# The theme, by name.\nname = \"Light\"\n"
    );

    // An existing string is replaced, its comments and the rest kept.
    let text = "# Mine.\n[theme]\nname = \"Light\" # chosen\n\n[editor]\n# Wide.\ntab_size = 8\n";
    fs::write(&file, text).unwrap();
    write_setting_str(&file, &["theme", "name"], "Dark Modern", TEMPLATE).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        text.replace("\"Light\"", "\"Dark Modern\"")
    );

    // A missing key goes under its table.
    write_setting_str(&file, &["theme", "dark"], "acme.night/night", TEMPLATE).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Mine.\n[theme]\nname = \"Dark Modern\" # chosen\ndark = \"acme.night/night\"\n\n\
         [editor]\n# Wide.\ntab_size = 8\n"
    );
    // And a missing table at the end.
    write_setting_str(&file, &["terminal", "shell"], "/bin/fish", TEMPLATE).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    assert!(
        written.ends_with("tab_size = 8\n\n[terminal]\nshell = \"/bin/fish\"\n"),
        "{written}"
    );

    // Quotes, backslashes and control characters are escaped.
    let odd = "say \"hi\" \\ there\n\ttab \u{1}";
    write_setting_str(&file, &["theme", "name"], odd, TEMPLATE).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    assert!(
        written.contains(r#"name = "say \"hi\" \\ there\n\ttab \u0001" # chosen"#),
        "{written}"
    );
    assert_eq!(string(&written, "theme", "name").as_deref(), Some(odd));
    assert_eq!(
        string(&written, "theme", "dark").as_deref(),
        Some("acme.night/night")
    );
}

#[test]
fn several_changes_are_one_write() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");

    // A key on its own line goes with its line; the comments around it stay.
    let text = "[theme]\n# Mine.\nname = \"Dark\" # chosen\ndark = \"acme.night/night\"\n";
    fs::write(&file, text).unwrap();
    let changes: &[(&[&str], _)] = &[
        (&["theme", "name"], None),
        (
            &["theme", "dark"],
            Some(SettingValue::Str("acme.dusk/dusk")),
        ),
    ];
    write_settings(&file, changes, TEMPLATE).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[theme]\n# Mine.\ndark = \"acme.dusk/dusk\"\n"
    );

    // A missing key removed changes nothing, and nothing is written.
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_settings(&file, &[(&["theme", "name"], None)], TEMPLATE).unwrap();
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    // Nor is a missing file, or its directory, made.
    let missing = dir.path().join("none/settings.toml");
    write_settings(&missing, &[(&["theme", "name"], None)], TEMPLATE).unwrap();
    assert!(!dir.path().join("none").exists());

    // One inside braces, or made of dotted keys, is removed too.
    fs::write(&file, "theme = { name = \"Dark\", dark = \"x/y\" }\n").unwrap();
    write_settings(&file, &[(&["theme", "name"], None)], TEMPLATE).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    assert_eq!(string(&written, "theme", "name"), None);
    assert_eq!(string(&written, "theme", "dark").as_deref(), Some("x/y"));
    fs::write(
        &file,
        "# Top.\ntheme.name = \"Dark\"\ntheme.mode = \"dark\"\n",
    )
    .unwrap();
    write_settings(&file, &[(&["theme", "name"], None)], TEMPLATE).unwrap();
    let written = fs::read_to_string(&file).unwrap();
    assert_eq!(string(&written, "theme", "name"), None);
    assert_eq!(string(&written, "theme", "mode").as_deref(), Some("dark"));
    assert!(written.starts_with("# Top.\n"), "{written}");

    // A bool and a string in one write, the string's commented default of
    // another type left as a comment.
    fs::write(&file, "[vim]\n# enabled = \"no\"\n").unwrap();
    let changes: &[(&[&str], _)] = &[
        (&["vim", "enabled"], Some(SettingValue::Bool(true))),
        (&["vim", "leader"], Some(SettingValue::Str(" "))),
    ];
    write_settings(&file, changes, TEMPLATE).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[vim]\nenabled = true\nleader = \" \"\n# enabled = \"no\"\n"
    );
}
