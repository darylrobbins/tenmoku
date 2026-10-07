//! `write_setting` adding a missing setting: where the new text goes, and
//! that every other byte stays.

use std::fs;

use fe_config::{SettingsStore, USER_SETTINGS_TEMPLATE, write_setting};

/// `original` with `key` set to `value`.
fn written(original: &str, key: &[&str], value: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    fs::write(&file, original).unwrap();
    write_setting(&file, key, value).unwrap();
    fs::read_to_string(&file).unwrap()
}

const VIM: [&str; 2] = ["vim", "enabled"];
const BAR: [&str; 2] = ["status_bar", "visible"];

/// Dotted keys of one table with another's between them don't round-trip
/// through toml_edit, but a setting is still added, and nothing else moves.
#[test]
fn adds_to_interleaved_dotted_keys() {
    let editor = "editor.tab_size = 2\n";
    let font = "editor.font_size = 15.0\n";
    for eol in ["\n", "\r\n"] {
        let fix = |text: &str| text.replace('\n', eol);
        let (editor, font) = (fix(editor), fix(font));

        let vim = fix("vim.enabled = false\n");
        let original = format!("{editor}{vim}{font}");
        let after = written(&original, &BAR, true);
        assert_eq!(
            after,
            format!("{original}{}", fix("\n[status_bar]\nvisible = true\n"))
        );

        let bar = fix("status_bar.visible = true\n");
        let original = format!("{editor}{bar}{font}");
        let after = written(&original, &VIM, true);
        assert_eq!(
            after,
            format!("{original}{}", fix("\n[vim]\nenabled = true\n"))
        );

        // Beside the table's own keys, wherever they are.
        let original = format!("{editor}{bar}{font}");
        let after = written(&original, &["editor", "soft_wrap"], true);
        assert_eq!(
            after,
            format!("{original}{}", fix("editor.soft_wrap = true\n"))
        );

        let store_file = tempfile::tempdir().unwrap();
        let path = store_file.path().join("settings.toml");
        fs::write(&path, &after).unwrap();
        let store = SettingsStore::load(Some(path), None);
        assert!(store.diagnostics().iter().all(|d| !d.is_error()));
    }
}

/// A table with only the template's commented default for the setting gets
/// the setting where that comment is.
#[test]
fn replaces_the_commented_default() {
    let template = USER_SETTINGS_TEMPLATE;
    for (key, value, comment, line) in [
        (VIM, true, "# enabled = false", "enabled = true"),
        (BAR, false, "# visible = true", "visible = false"),
    ] {
        let expected = template.replace(comment, line);
        assert_ne!(expected, template);
        assert_eq!(written("", &key, value), expected);
        assert_eq!(
            written("\u{feff}", &key, value),
            format!("\u{feff}{expected}")
        );
        // Not the template's file, but the same text.
        assert_eq!(written(template, &key, value), expected);
        let crlf = template.replace('\n', "\r\n");
        assert_eq!(written(&crlf, &key, value), expected.replace('\n', "\r\n"));
    }
}

/// Keys the table has already are kept, and the comment isn't taken then.
#[test]
fn a_table_with_keys_gets_the_setting_after_them() {
    let original = "[vim]\nother = 1 # mine\n# enabled = false\n\n[status_bar]\n";
    let after = written(original, &VIM, true);
    assert_eq!(
        after,
        "[vim]\nother = 1 # mine\nenabled = true\n# enabled = false\n\n[status_bar]\n"
    );
}

/// An inline table gets the setting inside its braces.
#[test]
fn adds_inside_an_inline_table() {
    assert_eq!(
        written("vim = { x = 1 } # mine\n", &VIM, true),
        "vim = { x = 1, enabled = true } # mine\n"
    );
    assert_eq!(
        written("vim = {}\n", &VIM, true),
        "vim = { enabled = true }\n"
    );
}

/// Names that aren't bare keys are quoted where they're added, so they
/// parse back as the one name.
#[test]
fn quotes_names_that_need_it() {
    let after = written("# Mine.\n", &["a.b", "c d", "e\"f"], true);
    assert_eq!(after, "# Mine.\n\n[\"a.b\".\"c d\"]\n'e\"f' = true\n");
    let table: toml::Table = after.parse().unwrap();
    assert_eq!(table["a.b"]["c d"]["e\"f"].as_bool(), Some(true));

    let dotted = written("\"a.b\".x = 1\n", &["a.b", "c d"], false);
    assert_eq!(dotted, "\"a.b\".x = 1\n\"a.b\".\"c d\" = false\n");
    let inline = written("\"a b\" = { x = 1 }\n", &["a b", "c.d"], true);
    assert_eq!(inline, "\"a b\" = { x = 1, \"c.d\" = true }\n");
    let nested = written("a = {}\n", &["a", "b c", "d"], true);
    assert_eq!(nested, "a = { \"b c\" = { d = true } }\n");
    let header = written("[\"a b\"]\nx = 1\n", &["a b", "c.d"], true);
    assert_eq!(header, "[\"a b\"]\nx = 1\n\"c.d\" = true\n");
    for text in [dotted, inline, nested, header] {
        assert!(text.parse::<toml::Table>().is_ok(), "{text:?}");
    }
}

/// Dotted keys inside braces (`{ x.y = 1 }`) are a table too: the setting
/// goes beside them, with their prefix, inside the same braces.
#[test]
fn adds_beside_dotted_keys_in_braces() {
    for (original, key, after) in [
        (
            "vim = { x.y = 1 }\n",
            &VIM[..],
            "vim = { x.y = 1, enabled = true }\n",
        ),
        (
            "outer = { vim.x = 1, z = 2 } # mine\n",
            &["outer", "vim", "enabled"][..],
            "outer = { vim.x = 1, vim.enabled = true, z = 2 } # mine\n",
        ),
        (
            "outer = { vim.x.y = 1 }\n",
            &["outer", "vim", "sub", "enabled"][..],
            "outer = { vim.x.y = 1, vim.sub.enabled = true }\n",
        ),
        (
            "outer = { vim.x = 1 }\n",
            &["outer", "other", "enabled"][..],
            "outer = { vim.x = 1, other = { enabled = true } }\n",
        ),
    ] {
        let text = written(original, key, true);
        assert_eq!(text, after);
        let table: toml::Table = text.parse().unwrap();
        let found = key
            .iter()
            .try_fold(toml::Value::Table(table), |at, part| at.get(part).cloned());
        assert_eq!(found.and_then(|v| v.as_bool()), Some(true), "{text:?}");
    }
}
