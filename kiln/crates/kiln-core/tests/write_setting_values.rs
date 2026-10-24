// SPDX-License-Identifier: LicenseRef-Proprietary

//! Writing integers, floats and string lists into a `settings.toml`, and
//! resetting a setting by removing its key, keeping the rest of the file.

use std::fs;

use kiln_core::write::{SettingValue, reset_settings, write_settings};

/// The commented-out defaults a missing file is created from.
const TEMPLATE: &str = "# Settings.\n\n[editor]\n# The font size.\n# font_size = 14\n";

fn set(file: &std::path::Path, key: &[&str], value: SettingValue<'_>) -> std::io::Result<()> {
    write_settings(file, &[(key, Some(value))], TEMPLATE)
}

#[test]
fn int_float_and_list_values_are_written() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");

    // A missing file starts from the template: the integer takes its
    // commented default's place.
    set(&file, &["editor", "font_size"], SettingValue::Int(123)).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Settings.\n\n[editor]\n# The font size.\nfont_size = 123\n"
    );

    set(&file, &["editor", "line_height"], SettingValue::Float(1.5)).unwrap();
    set(
        &file,
        &["editor", "rulers"],
        SettingValue::StrList(&["a", "b \"c\""]),
    )
    .unwrap();
    set(&file, &["editor", "none"], SettingValue::StrList(&[])).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Settings.\n\n[editor]\n# The font size.\nfont_size = 123\n\
         line_height = 1.5\nrulers = [\"a\", \"b \\\"c\\\"\"]\nnone = []\n"
    );

    let parsed: toml::Table = fs::read_to_string(&file).unwrap().parse().unwrap();
    let editor = parsed["editor"].as_table().unwrap();
    assert_eq!(editor["font_size"].as_integer(), Some(123));
    assert_eq!(editor["line_height"].as_float(), Some(1.5));
    let rulers: Vec<&str> = (editor["rulers"].as_array().unwrap().iter())
        .map(|item| item.as_str().unwrap())
        .collect();
    assert_eq!(rulers, ["a", "b \"c\""]);

    // A negative integer, replacing a value in place.
    set(&file, &["editor", "font_size"], SettingValue::Int(-7)).unwrap();
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("\nfont_size = -7\n")
    );
}

#[test]
fn float_keeps_a_point() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    fs::write(&file, "[editor]\nscale = 1.0\n").unwrap();

    for (value, text) in [
        (13.0, "13.0"),
        (1.5, "1.5"),
        (-0.25, "-0.25"),
        (0.0, "0.0"),
        (1e300, "1e300"),
        (1e-7, "1e-7"),
    ] {
        set(&file, &["editor", "scale"], SettingValue::Float(value)).unwrap();
        let written = fs::read_to_string(&file).unwrap();
        assert_eq!(written, format!("[editor]\nscale = {text}\n"));
        let parsed: toml::Table = written.parse().unwrap();
        assert_eq!(parsed["editor"]["scale"].as_float(), Some(value), "{text}");
    }
}

#[test]
fn comments_and_order_survive() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    let text = "# Mine.\n\n[editor]\n# Big.\nfont_size = 14 # pt\n\
                tab_size = 4\n\n# Vim.\n[vim]\nenabled = true\n";
    fs::write(&file, text).unwrap();

    let changes: &[(&[&str], _)] = &[
        (&["editor", "font_size"], Some(SettingValue::Int(16))),
        (&["editor", "zoom"], Some(SettingValue::Float(2.0))),
        (&["vim", "keys"], Some(SettingValue::StrList(&["jk"]))),
    ];
    write_settings(&file, changes, TEMPLATE).unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Mine.\n\n[editor]\n# Big.\nfont_size = 16 # pt\n\
         tab_size = 4\nzoom = 2.0\n\n# Vim.\n[vim]\nenabled = true\nkeys = [\"jk\"]\n"
    );

    // Bools and strings write as they did.
    let changes: &[(&[&str], _)] = &[
        (&["vim", "enabled"], Some(SettingValue::Bool(false))),
        (&["vim", "leader"], Some(SettingValue::Str(" "))),
    ];
    write_settings(&file, changes, TEMPLATE).unwrap();
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .ends_with("[vim]\nenabled = false\nkeys = [\"jk\"]\nleader = \" \"\n")
    );
}

#[test]
fn non_finite_float_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    fs::write(&file, "[editor]\nscale = 1.0\n").unwrap();

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let error = set(&file, &["editor", "scale"], SettingValue::Float(value)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{value}");
    }
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[editor]\nscale = 1.0\n"
    );
    // Nor is a missing file made.
    let missing = dir.path().join("none/settings.toml");
    let error = set(&missing, &["a"], SettingValue::Float(f64::NAN)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!dir.path().join("none").exists());
}

#[test]
fn reset_prunes_the_tables_it_empties() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    let reset = |text: &str, key: &[&str]| {
        fs::write(&file, text).unwrap();
        reset_settings(&file, &[key], TEMPLATE).unwrap();
        fs::read_to_string(&file).unwrap()
    };

    // The last table goes with the blank lines before it.
    assert_eq!(
        reset(
            "a = 1\n\n[editor]\nfont_size = 14\n",
            &["editor", "font_size"]
        ),
        "a = 1\n"
    );
    // One in the middle goes with the blank lines under it.
    assert_eq!(
        reset(
            "[editor]\nfont_size = 14\n\n[vim]\nenabled = true\n",
            &["editor", "font_size"]
        ),
        "[vim]\nenabled = true\n"
    );
    // A nested one, then the header over it it leaves empty.
    assert_eq!(
        reset(
            "[vim]\nenabled = true\n\n[editor]\n\n[editor.minimap]\nenabled = true\n",
            &["editor", "minimap", "enabled"]
        ),
        "[vim]\nenabled = true\n"
    );
    // Inline tables, and dotted keys.
    assert_eq!(
        reset(
            "[editor]\nminimap = { enabled = true }\n\n[vim]\nx = 1\n",
            &["editor", "minimap", "enabled"]
        ),
        "[vim]\nx = 1\n"
    );
    assert_eq!(
        reset("a = 1\neditor.font_size = 14\n", &["editor", "font_size"]),
        "a = 1\n"
    );
    // CRLF.
    assert_eq!(
        reset(
            "a = 1\r\n\r\n[editor]\r\nfont_size = 14\r\n",
            &["editor", "font_size"]
        ),
        "a = 1\r\n"
    );

    // A table with another key, or a comment, stays.
    assert_eq!(
        reset(
            "[editor]\nfont_size = 14\ntab_size = 2\n",
            &["editor", "font_size"]
        ),
        "[editor]\ntab_size = 2\n"
    );
    assert_eq!(
        reset(
            "[editor]\n# Mine.\nfont_size = 14\n",
            &["editor", "font_size"]
        ),
        "[editor]\n# Mine.\n"
    );
    assert_eq!(
        reset(
            "[editor] # Mine.\nfont_size = 14\n",
            &["editor", "font_size"]
        ),
        "[editor] # Mine.\n"
    );
    assert_eq!(
        reset(
            "# The editor.\n[editor]\nfont_size = 14\n",
            &["editor", "font_size"]
        ),
        "# The editor.\n[editor]\n"
    );
    assert_eq!(
        reset(
            "[editor]\n\n[editor.minimap]\nenabled = true\n\n[editor.x]\ny = 1\n",
            &["editor", "minimap", "enabled"]
        ),
        "[editor]\n\n[editor.x]\ny = 1\n"
    );

    // So does an inline table with a comment after it, before it, or, in
    // one written over several lines, inside it.
    assert_eq!(
        reset(
            "editor = { font_size = 14 } # keep editor\n",
            &["editor", "font_size"]
        ),
        "editor = {} # keep editor\n"
    );
    assert_eq!(
        reset(
            "# The editor.\neditor = { font_size = 14 }\n",
            &["editor", "font_size"]
        ),
        "# The editor.\neditor = {}\n"
    );
    assert_eq!(
        reset(
            "editor = {\n  # keep editor\n  font_size = 14,\n}\n",
            &["editor", "font_size"]
        ),
        "editor = {\n  # keep editor\n}\n"
    );
    assert_eq!(
        reset(
            "editor = {\n  font_size = 14, # pt\n  # keep editor\n}\n",
            &["editor", "font_size"]
        ),
        "editor = {\n  # keep editor\n}\n"
    );
    // The closing brace on the removed value's line, nested tables too.
    assert_eq!(
        reset(
            "editor = {\n  # keep editor\n  font_size = 14 }\n",
            &["editor", "font_size"]
        ),
        "editor = {\n  # keep editor\n   }\n"
    );
    assert_eq!(
        reset(
            "editor = { minimap = {\n  # keep\n  enabled = true } }\n",
            &["editor", "minimap", "enabled"]
        ),
        "editor = { minimap = {\n  # keep\n   } }\n"
    );
    assert_eq!(
        reset(
            "editor = {\n  # keep editor\n  tab_size = 2,\n  minimap = { enabled = true } }\n",
            &["editor", "minimap", "enabled"]
        ),
        "editor = {\n  # keep editor\n  tab_size = 2 }\n"
    );
    assert_eq!(
        reset("editor = {\n  a = 1, # one\n  b = 2 }\n", &["editor", "b"]),
        "editor = {\n  a = 1 # one\n   }\n"
    );
    // A key among others in braces takes one comma with it.
    for (key, expected) in [
        ("a", "editor = { b = 2, c = 3 }\n"),
        ("b", "editor = { a = 1, c = 3 }\n"),
        ("c", "editor = { a = 1, b = 2 }\n"),
    ] {
        assert_eq!(
            reset("editor = { a = 1, b = 2, c = 3 }\n", &["editor", key]),
            expected
        );
    }
    // A `#` in a string isn't a comment.
    assert_eq!(
        reset(
            "editor = { minimap = { enabled = true }, font_family = \"F# Code\" }\n",
            &["editor", "minimap", "enabled"]
        ),
        "editor = { font_family = \"F# Code\" }\n"
    );
    assert_eq!(
        reset("a = 1\n\n[\"x#y\"]\nk = 1\n", &["x#y", "k"]),
        "a = 1\n"
    );
    assert_eq!(
        reset(
            "s = \"\"\"\n# not a comment\n\"\"\"\n[editor]\nk = 1\n",
            &["editor", "k"]
        ),
        "s = \"\"\"\n# not a comment\n\"\"\"\n"
    );
    assert_eq!(
        reset(
            "s = 'a#b'\nt = \"a\\\"#\"\n[editor]\nk = 1\n",
            &["editor", "k"]
        ),
        "s = 'a#b'\nt = \"a\\\"#\"\n"
    );

    // One left with no comment goes.
    assert_eq!(
        reset(
            "a = 1\neditor = {\n  font_size = 14,\n}\n",
            &["editor", "font_size"]
        ),
        "a = 1\n"
    );

    // A missing key writes nothing, and leaves an empty table be.
    fs::write(&file, "[editor]\n").unwrap();
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    reset_settings(&file, &[&["editor", "font_size"]], TEMPLATE).unwrap();
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_to_string(&file).unwrap(), "[editor]\n");
    let missing = dir.path().join("none/settings.toml");
    reset_settings(&missing, &[&["editor", "font_size"]], TEMPLATE).unwrap();
    assert!(!dir.path().join("none").exists());
}

#[test]
fn a_table_added_to_an_empty_file_has_nothing_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    write_settings(
        &file,
        &[(&["editor", "font_size"], Some(SettingValue::Int(12)))],
        "",
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[editor]\nfont_size = 12\n"
    );
}

#[test]
fn removing_a_key_never_drops_a_comment() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    let text = "editor = {\n  # keep editor\n  minimap.enabled = true }\n";
    let key: &[&str] = &["editor", "minimap", "enabled"];

    fs::write(&file, text).unwrap();
    let error = reset_settings(&file, &[key], TEMPLATE).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported, "{error}");
    let error = write_settings(&file, &[(key, None)], TEMPLATE).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported, "{error}");
    assert_eq!(fs::read_to_string(&file).unwrap(), text);

    // Without the comment, it goes.
    fs::write(&file, "editor = { minimap.enabled = true, x = 1 }\n").unwrap();
    write_settings(&file, &[(key, None)], TEMPLATE).unwrap();
    let parsed: toml::Table = fs::read_to_string(&file).unwrap().parse().unwrap();
    assert_eq!(parsed["editor"].as_table().unwrap().len(), 1);
}
