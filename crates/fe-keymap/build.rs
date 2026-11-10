//! Resolves fastedit's one keymap and one menu table for macOS and Linux,
//! at the TOML level, into `OUT_DIR`: the text behind `DEFAULT_MACOS`,
//! `DEFAULT_LINUX`, `DEFAULT_MACOS_MENUS` and `DEFAULT_LINUX_MENUS`. The
//! rules are `KeymapLayer::parse_for`'s (kiln-core's `keymap` module):
//!
//! - an entry whose `platform` excludes the OS is dropped, and `platform`
//!   removed from those kept;
//! - `keys = { macos = .., default = .. }` becomes the OS's string, its
//!   `default` for an OS not named, the entry dropped with neither;
//! - `secondary` in a keystroke is `cmd` on macOS and `ctrl` elsewhere.
//!
//! Anything else in an entry is copied as it is, so each copy names its
//! titles by the same `title_key`s as the table (this crate's catalogue,
//! `l10n/`), never by literal text: `tests/l10n.rs` checks the copies too.

use std::{env, fs, path::Path};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

#[derive(Clone, Copy, PartialEq)]
enum Os {
    MacOs,
    Linux,
}

const OSES: [&str; 3] = ["macos", "linux", "windows"];

impl Os {
    fn name(self) -> &'static str {
        match self {
            Os::MacOs => "macos",
            Os::Linux => "linux",
        }
    }
}

fn main() {
    let tables = [
        ("keymaps/fastedit.toml", "keymap"),
        ("menus/fastedit.toml", "menus"),
    ];
    println!("cargo:rerun-if-changed=build.rs");
    let out = env::var_os("OUT_DIR").expect("OUT_DIR");
    for (path, stem) in tables {
        println!("cargo:rerun-if-changed={path}");
        let source = fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        for os in [Os::MacOs, Os::Linux] {
            let mut document: DocumentMut = source
                .parse()
                .unwrap_or_else(|e| panic!("{path}: not TOML: {e}"));
            resolve_document(document.as_table_mut(), path, os);
            let target = Path::new(&out).join(format!("{stem}-{}.toml", os.name()));
            fs::write(&target, document.to_string())
                .unwrap_or_else(|e| panic!("{}: {e}", target.display()));
        }
    }
}

/// Resolves the entries `table` holds: `[[bindings]]`, `[[menus]]` and
/// their `[[menus.items]]`, and no others (an entry's `args` are its own).
fn resolve_document(table: &mut Table, path: &str, os: Os) {
    if let Some(entries) = table
        .get_mut("bindings")
        .and_then(Item::as_array_of_tables_mut)
    {
        resolve_entries(entries, &format!("{path}: [[bindings]]"), os);
    }
    if let Some(menus) = table
        .get_mut("menus")
        .and_then(Item::as_array_of_tables_mut)
    {
        // Items first, so those of a menu left out are checked too.
        for menu in menus.iter_mut() {
            if let Some(items) = menu.get_mut("items").and_then(Item::as_array_of_tables_mut) {
                resolve_entries(items, &format!("{path}: [[menus.items]]"), os);
            }
        }
        resolve_entries(menus, &format!("{path}: [[menus]]"), os);
    }
}

/// Every entry is checked for every OS before any is left out, so a mistake
/// in one that's excluded still fails the build.
fn resolve_entries(entries: &mut ArrayOfTables, at: &str, os: Os) {
    let keep: Vec<bool> = entries
        .iter_mut()
        .enumerate()
        .map(|(ix, entry)| resolve_entry(entry, &format!("{at} entry {}", ix + 1), os))
        .collect();
    let mut ix = 0;
    entries.retain(|_| {
        ix += 1;
        keep[ix - 1]
    });
}

/// Resolves `entry` for `os`; false if it's left out there. A mistake in
/// `platform` or `keys` panics whatever `os` is.
fn resolve_entry(entry: &mut Table, at: &str, os: Os) -> bool {
    let mut present = true;
    if let Some(item) = entry.remove("platform") {
        let filter = item
            .as_str()
            .unwrap_or_else(|| panic!("{at}: platform isn't a string"));
        present = filter_matches(filter, os, at);
    }
    let Some(keys) = entry.get("keys") else {
        return present;
    };
    let keys = match keys {
        Item::Value(Value::String(keys)) => Some(keys.value().clone()),
        Item::Value(Value::InlineTable(by_os)) => {
            for (name, value) in by_os.iter() {
                if name != "default" && !OSES.contains(&name) {
                    panic!("{at}: unknown platform {name:?} in keys");
                }
                if value.as_str().is_none() {
                    panic!("{at}: keys for {name} isn't a string");
                }
            }
            (by_os.get(os.name()).or_else(|| by_os.get("default")))
                .map(|keys| keys.as_str().expect("checked above").to_owned())
        }
        _ => panic!("{at}: keys isn't a string or a table of strings"),
    };
    match keys {
        Some(keys) if present => {
            entry["keys"] = Item::Value(secondary(&keys, os).into());
            true
        }
        _ => false,
    }
}

/// `PlatformFilter::matches`: negations exclude; names include, and a filter
/// with no names includes everything not excluded.
fn filter_matches(filter: &str, os: Os, at: &str) -> bool {
    let (mut include, mut exclude) = (Vec::new(), Vec::new());
    for part in filter.split(',').map(str::trim) {
        let (name, list) = match part.strip_prefix('!') {
            Some(name) => (name.trim(), &mut exclude),
            None => (part, &mut include),
        };
        if !OSES.contains(&name) {
            panic!("{at}: unknown platform {name:?}: expected macos, linux or windows");
        }
        list.push(name);
    }
    !exclude.contains(&os.name()) && (include.is_empty() || include.contains(&os.name()))
}

/// `keys` with each keystroke's `secondary` modifier as `os`'s.
fn secondary(keys: &str, os: Os) -> String {
    let own = if os == Os::MacOs { "cmd" } else { "ctrl" };
    keys.split_whitespace()
        .map(|stroke| {
            // The key is after the last `-`, unless that `-` is the key.
            let (modifiers, key) = if stroke == "-" {
                return stroke.to_owned();
            } else if let Some(modifiers) = stroke.strip_suffix("--") {
                (modifiers, "-")
            } else if let Some(split) = stroke.rsplit_once('-') {
                split
            } else {
                return stroke.to_owned();
            };
            let modifiers: Vec<_> = modifiers
                .split('-')
                .map(|m| {
                    if m.eq_ignore_ascii_case("secondary") {
                        own
                    } else {
                        m
                    }
                })
                .collect();
            format!("{}-{key}", modifiers.join("-"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}
