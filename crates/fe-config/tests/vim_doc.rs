//! docs/vim.md keeps up with `[vim]`: its Settings table has a row for each
//! key `Settings::default()` serialises under `[vim]`, with that default, and
//! a row for no other key.

use std::{collections::BTreeMap, fs, path::Path};

use fe_config::Settings;

const SECTIONS: [&str; 7] = [
    "Enabling",
    "Settings",
    "Modes and editing",
    "Ex commands",
    "Emulated plugins",
    "Keymaps",
    "Differences from Neovim",
];

fn doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/vim.md");
    fs::read_to_string(&path).unwrap()
}

/// The lines of the `## {name}` section, up to the next `## ` heading.
fn section<'a>(doc: &'a str, name: &str) -> Vec<&'a str> {
    let heading = format!("## {name}");
    doc.lines()
        .skip_while(|line| *line != heading)
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .collect()
}

/// The text between the first pair of backticks in `cell`.
fn code(cell: &str) -> Option<&str> {
    let (_, rest) = cell.split_once('`')?;
    Some(rest.split_once('`')?.0)
}

/// The Settings table's rows: each key with its default, as written.
fn rows(doc: &str) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    for line in section(doc, "Settings") {
        let Some(row) = line.strip_prefix('|') else {
            continue;
        };
        let cells: Vec<&str> = row.split(" | ").map(str::trim).collect();
        // The header and the rule under it.
        let Some(key) = code(cells[0]) else {
            continue;
        };
        let default = cells
            .get(2)
            .and_then(|cell| code(cell))
            .unwrap_or_else(|| panic!("`{key}`'s row has no default: {line}"));
        let earlier = rows.insert(key.to_string(), default.to_string());
        assert_eq!(earlier, None, "`{key}` has two rows");
    }
    rows
}

#[test]
fn has_every_section() {
    let doc = doc();
    for name in SECTIONS {
        assert!(
            doc.lines().any(|line| line == format!("## {name}")),
            "no `## {name}` section"
        );
    }
    assert!(
        doc.lines()
            .any(|line| line == "### Unsupported ex commands"),
        "no `### Unsupported ex commands` heading"
    );
}

#[test]
fn settings_table_has_every_vim_key_and_its_default() {
    let defaults = toml::Table::try_from(Settings::default()).unwrap();
    let vim = defaults["vim"].as_table().unwrap();
    let rows = rows(&doc());

    for (key, value) in vim {
        let default = rows
            .get(key)
            .unwrap_or_else(|| panic!("`[vim] {key}` has no row in docs/vim.md's Settings"));
        // Read as TOML, so `"\\"` and `'\'` are the same default.
        let written: toml::Table = toml::from_str(&format!("value = {default}"))
            .unwrap_or_else(|err| panic!("`[vim] {key}`'s default `{default}`: {err}"));
        assert_eq!(written["value"], *value, "`[vim] {key}`'s default");
    }
    for key in rows.keys() {
        assert!(
            vim.contains_key(key),
            "docs/vim.md's Settings has a row for `{key}`, which isn't a `[vim]` key"
        );
    }
}

/// The spans between backticks in `text`, in order.
fn spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

#[test]
fn cited_files_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc = doc();
    let mut cited = 0;
    for line in doc.lines() {
        // The plugin table's paths are under crates/fe-vim.
        let plugin_row =
            line.starts_with("| ") && section(&doc, "Emulated plugins").contains(&line);
        for span in spans(line) {
            let path = match span {
                _ if span.contains(' ') || span.contains('{') => continue,
                _ if span.starts_with("crates/")
                    || span.starts_with("docs/")
                    || span == "CLEAN_ROOM.md" =>
                {
                    root.join(span)
                }
                _ if plugin_row && (span.starts_with("src/") || span.starts_with("tests/")) => {
                    root.join("crates/fe-vim").join(span)
                }
                _ => continue,
            };
            assert!(
                path.exists(),
                "docs/vim.md cites `{span}`, which isn't there"
            );
            cited += 1;
        }
    }
    assert!(cited > 80, "only {cited} cited paths");
}

#[test]
fn plugins_not_implemented_are_said_so() {
    let doc = doc();
    let plugins = section(&doc, "Emulated plugins").join("\n");
    let (_, missing) = plugins
        .split_once("**Not implemented:**")
        .expect("no **Not implemented:** paragraph");
    for name in ["easymotion", "vim-sneak", "targets.vim"] {
        assert!(
            missing.contains(name),
            "{name} isn't said to be not implemented"
        );
    }
    assert!(!doc.contains("T910"), "docs/vim.md names T910");
}
