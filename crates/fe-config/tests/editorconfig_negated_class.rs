//! One-character negated classes in `.editorconfig` section globs.

use std::fs;

use fe_config::{IndentSize, editorconfig_for};

#[test]
fn one_character_negated_classes_match_any_other_character() {
    let cases: &[(&str, &str, bool)] = &[
        ("[!_]*.py", "a.py", true),
        ("[!_]*.py", "_a.py", false),
        ("*.[!c]", "a.h", true),
        ("*.[!c]", "a.c", false),
        ("[!a]", "b", true),
        ("[!a]", "a", false),
        ("*.[!-]", "a.x", true),
        ("*.[!\\]]", "a.x", true),
        ("*.[!ch]", "a.x", true),
        ("*.[!ch]", "a.h", false),
        ("[c]", "c", true),
        ("!a", "!a", true),
        ("!a", "b", false),
        ("[a[!-]", "-", true),
        ("[a[!-]", "!", true),
        ("[a[!-]", "b", false),
        ("[!ab[!-]", "-", false),
        ("[!ab[!-]", "c", true),
    ];
    for (glob, file, applies) in cases {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".editorconfig"),
            format!("root = true\n\n[{glob}]\nindent_size = 3\n"),
        )
        .unwrap();
        let got = editorconfig_for(&dir.path().join(file)).indent_size;
        let want = applies.then_some(IndentSize::Columns(3));
        assert_eq!(got, want, "[{glob}] on {file}");
    }
}
