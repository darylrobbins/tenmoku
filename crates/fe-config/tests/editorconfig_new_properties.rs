//! `.editorconfig` `charset` and `max_line_length`.

use std::fs;

use fe_config::editorconfig::{Charset, MaxLineLength};
use fe_config::editorconfig_for;

/// Each value set for `*.<name>` files, looked up for `a.<name>`.
fn lookup<T>(key: &str, values: &[&str], read: impl Fn(fe_config::EditorConfig) -> T) -> Vec<T> {
    let dir = tempfile::tempdir().unwrap();
    let mut config = String::from("root = true\n");
    for (i, value) in values.iter().enumerate() {
        config.push_str(&format!("[*.e{i}]\n{key} = {value}\n"));
    }
    fs::write(dir.path().join(".editorconfig"), config).unwrap();
    (0..values.len())
        .map(|i| read(editorconfig_for(&dir.path().join(format!("a.e{i}")))))
        .collect()
}

#[test]
fn charset_values() {
    let values = [
        "latin1",
        "utf-8",
        "UTF-8-BOM",
        "utf-16be",
        "utf-16le",
        "utf-32",
        "cp1252",
        "unset",
    ];
    assert_eq!(
        lookup("charset", &values, |c| c.charset),
        [
            Some(Charset::Latin1),
            Some(Charset::Utf8),
            Some(Charset::Utf8Bom),
            Some(Charset::Utf16Be),
            Some(Charset::Utf16Le),
            None,
            None,
            None,
        ]
    );

    // Missing.
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(editorconfig_for(&dir.path().join("a.txt")).charset, None);

    // A nearer file wins over its parent.
    let top = dir.path();
    fs::write(
        top.join(".editorconfig"),
        "root = true\n[*]\ncharset = latin1\n",
    )
    .unwrap();
    fs::create_dir(top.join("sub")).unwrap();
    fs::write(top.join("sub/.editorconfig"), "[*]\ncharset = utf-16le\n").unwrap();
    assert_eq!(
        editorconfig_for(&top.join("a.txt")).charset,
        Some(Charset::Latin1)
    );
    assert_eq!(
        editorconfig_for(&top.join("sub/a.txt")).charset,
        Some(Charset::Utf16Le)
    );
}

#[test]
fn max_line_length_values() {
    let values = ["80", "off", "OFF", "0", "-1", "wide", "unset"];
    assert_eq!(
        lookup("max_line_length", &values, |c| c.max_line_length),
        [
            Some(MaxLineLength::Columns(80)),
            Some(MaxLineLength::Off),
            Some(MaxLineLength::Off),
            None,
            None,
            None,
            None,
        ]
    );
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        editorconfig_for(&dir.path().join("a.txt")).max_line_length,
        None
    );
}
