//! Which-key's model (`VimState::which_key`): the keys that go on from the
//! reducer's `g`, `z`, `Z`, `[` and `]`, and from the keys the remap engine
//! holds, with their titles; checked against the reducer through
//! `HeadlessVim` so the built-in table can't drift from what it takes.

use std::sync::Arc;

use fe_vim::{
    BUILTIN_KEYS, HeadlessVim, Remap, RemapMode, RemapSet, Step, VimKey, WhichKey, WhichKeyEntry,
    WhichKeyLabel, parse_keys,
};

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;

const TEXT: &str = "one two (three)\nfour {five}\nsix\n";

fn map(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

fn vim(remaps: &[Remap], leader: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    let (set, issues) = RemapSet::new(remaps, leader);
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

fn keys(text: &str) -> Vec<VimKey> {
    parse_keys(text).unwrap()
}

fn listed(which: &WhichKey) -> Vec<&str> {
    which
        .entries
        .iter()
        .map(|entry| entry.key.as_str())
        .collect()
}

fn entry<'a>(which: &'a WhichKey, key: &str) -> &'a WhichKeyEntry {
    (which.entries.iter().find(|entry| entry.key == key))
        .unwrap_or_else(|| panic!("{key} not listed in {which:?}"))
}

/// Every key a test tries after a prefix: the printable ASCII, the ctrl
/// letters and the named keys.
fn every_key() -> Vec<VimKey> {
    let mut keys: Vec<VimKey> = (b' '..=b'~').map(|b| VimKey::Char(b as char)).collect();
    keys.extend(('a'..='z').map(VimKey::Ctrl));
    keys.extend([
        VimKey::Esc,
        VimKey::Enter,
        VimKey::Backspace,
        VimKey::Tab,
        VimKey::Left,
        VimKey::Right,
        VimKey::Up,
        VimKey::Down,
    ]);
    keys
}

/// Whether the reducer takes `key` after `prefix`, in normal mode or (if
/// `visual`) charwise visual: what `HeadlessVim`'s state answers it, short
/// of `Cancel` (or a forward).
fn takes(visual: bool, prefix: char, key: VimKey) -> bool {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.set_replace_with_register(true);
    if visual {
        vim.feed("v").unwrap();
    }
    if vim.feed(&prefix.to_string()).is_err() {
        return false;
    }
    !matches!(vim.state_mut().feed(key), Step::Cancel | Step::Forward)
}

#[test]
fn which_key_lists_builtin_prefixes() {
    let cases: &[(&str, char, &[&str])] = &[
        (
            "",
            'g',
            &["g", "e", "E", "j", "k", "s", "v", "i", "&", "u", "U"],
        ),
        (
            "v",
            'g',
            &["g", "e", "E", "j", "k", "s", "v", "<C-a>", "<C-x>"],
        ),
        (
            "",
            'z',
            &["z", "t", "b", "<CR>", "o", "c", "a", "f", "j", "k"],
        ),
        ("v", 'z', &["z", "t", "f", "j", "k"]),
        ("", 'Z', &["Z", "Q"]),
        ("", '[', &["(", "{", "c", "p", "P"]),
        ("v", '[', &["(", "{", "c"]),
        ("", ']', &[")", "}", "c", "p", "P"]),
        ("v", ']', &[")", "}", "c"]),
    ];
    for &(before, prefix, expected) in cases {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed(before).unwrap();
        vim.feed(&prefix.to_string()).unwrap();
        let which =
            (vim.state().which_key()).unwrap_or_else(|| panic!("nothing after {before}{prefix}"));
        assert_eq!(which.prefix, prefix.to_string());
        for key in expected {
            entry(&which, key);
        }
        for entry in &which.entries {
            match entry.label {
                WhichKeyLabel::Builtin(title) => {
                    assert!(!title.is_empty(), "{before}{prefix}{}", entry.key)
                }
                ref label => panic!("{before}{prefix}{}: {label:?}", entry.key),
            }
            assert!(!entry.group);
        }
    }

    // Only what the reducer takes there: `g&` and `gi` in normal mode,
    // `g<C-a>` in a visual one, `[p` in normal mode.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("vg").unwrap();
    let visual_g = vim.state().which_key().unwrap();
    assert!(!listed(&visual_g).contains(&"&"));
    assert!(!listed(&visual_g).contains(&"i"));
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("g").unwrap();
    let normal_g = vim.state().which_key().unwrap();
    assert!(!listed(&normal_g).contains(&"<C-a>"));
    // `gr` is the host's while ReplaceWithRegister is off.
    assert!(!listed(&normal_g).contains(&"r"));
    vim.set_replace_with_register(true);
    assert!(listed(&vim.state().which_key().unwrap()).contains(&"r"));

    // A count before the prefix, or a register, still lists it.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("3\"ag").unwrap();
    assert_eq!(vim.state().which_key().unwrap().entries, normal_g.entries);

    // `Z` is no prefix in a visual mode.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("v").unwrap();
    assert!(vim.feed("Z").is_err());
    assert_eq!(vim.state().which_key(), None);

    // Sorted case aside, a lowercase key before its uppercase.
    let at = |key| listed(&normal_g).iter().position(|k| *k == key).unwrap();
    assert!(at("e") + 1 == at("E"));
    assert!(at("E") < at("g") && at("g") < at("i"));
    assert!(at("u") + 1 == at("U"));
}

#[test]
fn which_key_lists_remap_continuations() {
    let remaps = [
        map(&[N], "<leader>ff", "<Cmd>fe:file.find<CR>"),
        map(&[N], "<leader>fr", ":browse oldfiles<CR>"),
        map(&[N], "<leader>w", "<Cmd>fe:file.save<CR>"),
        map(&[N], "<leader>t", "x"),
        map(&[N], "<leader>tt", "dd"),
        map(&[N], "<leader>a", "x<Cmd>fe:file.save<CR>"),
        map(&[N], "gj", "<Cmd>fe:view.down<CR>"),
        map(&[X], "<leader>c", "y"),
    ];
    let mut vim = vim(&remaps, "\\");
    vim.feed("\\").unwrap();
    assert!(vim.state().remap_waiting());
    assert_eq!(vim.state().remap_held(), keys("\\"));
    let which = vim.state().which_key().unwrap();
    assert_eq!(which.prefix, "\\");
    assert_eq!(listed(&which), ["a", "w", "f", "t"]);
    let f = entry(&which, "f");
    assert_eq!(
        (&f.label, f.group),
        (
            &WhichKeyLabel::Group {
                count: 2,
                name: None
            },
            true
        )
    );
    let w = entry(&which, "w");
    let save = WhichKeyLabel::Command("file.save".into());
    assert_eq!((&w.label, w.group), (&save, false));
    // A map that's also a prefix: its own label, a group.
    let t = entry(&which, "t");
    assert_eq!(
        (&t.label, t.group),
        (&WhichKeyLabel::Keys("x".into()), true)
    );
    // A right side with more than a `<Cmd>fe:…<CR>`: the keys as written.
    let a = entry(&which, "a");
    let written = WhichKeyLabel::Keys("x<Cmd>fe:file.save<CR>".into());
    assert_eq!(a.label, written);

    vim.feed("f").unwrap();
    let which = vim.state().which_key().unwrap();
    assert_eq!(which.prefix, "\\f");
    assert_eq!(listed(&which), ["f", "r"]);
    let find = WhichKeyLabel::Command("file.find".into());
    assert_eq!(entry(&which, "f").label, find);
    let oldfiles = WhichKeyLabel::Keys(":browse oldfiles<CR>".into());
    assert_eq!(entry(&which, "r").label, oldfiles);

    // A user map wins over a built-in title for the same key; the rest of
    // `g`'s keys still list.
    let mut vim = self::vim(&remaps, "\\");
    vim.feed("g").unwrap();
    let which = vim.state().which_key().unwrap();
    assert_eq!(which.prefix, "g");
    let down = WhichKeyLabel::Command("view.down".into());
    assert_eq!(entry(&which, "j").label, down);
    assert_eq!(
        entry(&which, "g").label,
        WhichKeyLabel::Builtin("first line")
    );
    assert_eq!(listed(&which).iter().filter(|key| **key == "j").count(), 1);

    // A visual mode lists its own maps.
    let mut vim = self::vim(&remaps, "\\");
    vim.feed("v\\").unwrap();
    let which = vim.state().which_key().unwrap();
    assert_eq!(listed(&which), ["c"]);

    // A host that holds the keys itself.
    let vim = self::vim(&remaps, "\\");
    let which = vim.state().which_key_for(&keys("\\f")).unwrap();
    assert_eq!(
        (which.prefix.as_str(), listed(&which)),
        ("\\f", vec!["f", "r"])
    );

    // A `<Space>` leader is named so.
    let mut vim = self::vim(&remaps, "<Space>");
    vim.feed("<Space>").unwrap();
    let which = vim.state().which_key().unwrap();
    assert_eq!(which.prefix, "<Space>");
    assert_eq!(listed(&which), ["a", "w", "f", "t"]);

    // After a count, the maps still list.
    let mut vim = self::vim(&remaps, "\\");
    vim.feed("2\\").unwrap();
    assert_eq!(
        listed(&vim.state().which_key().unwrap()),
        ["a", "w", "f", "t"]
    );
}

#[test]
fn which_key_is_none_without_continuations() {
    let remaps = [
        map(&[N], "<leader>w", "x"),
        map(&[RemapMode::OperatorPending], "<leader>l", "$"),
    ];
    let none = |typed: &str| {
        let mut vim = vim(&remaps, "\\");
        vim.feed(typed).unwrap();
        assert_eq!(vim.state().which_key(), None, "after {typed:?}");
    };
    none("");
    none("v");
    // A count or a register alone.
    none("3");
    none("\"a");
    none("2\"b");
    // Operator-pending: an operator, its `g` or `z`, its maps.
    none("d");
    none("y");
    none("c");
    none("3d");
    none("dg");
    none("dz");
    none("d\\");
    // Another prefix.
    none("f");
    none("m");
    none("gs");
    // A complete leaf map.
    none("\\w");
    // Insert mode.
    none("i");

    // A host's keys that start no longer map.
    let vim = vim(&remaps, "\\");
    assert_eq!(vim.state().which_key_for(&keys("\\w")), None);
    assert_eq!(vim.state().which_key_for(&keys("q")), None);
}

#[test]
fn which_key_table_keys_are_real() {
    let keys = every_key();
    for table in BUILTIN_KEYS {
        for &(key, title) in table.keys {
            assert!(!title.is_empty());
            let modes: Vec<bool> = [false, true]
                .into_iter()
                .filter(|&visual| takes(visual, table.prefix, key))
                .collect();
            assert!(
                !modes.is_empty(),
                "{}{key:?} is in the table but the reducer cancels it",
                table.prefix
            );
        }
        // And the other way: every key the reducer takes has a title.
        for visual in [false, true] {
            for &key in &keys {
                if takes(visual, table.prefix, key) {
                    assert!(
                        table.keys.iter().any(|&(k, _)| k == key),
                        "{}{key:?} (visual: {visual}) has no title",
                        table.prefix
                    );
                }
            }
        }
    }
    // Which-key lists exactly what the reducer takes, mode by mode.
    for table in BUILTIN_KEYS {
        for visual in [false, true] {
            let mut vim = HeadlessVim::new(TEXT, (0, 0));
            vim.set_replace_with_register(true);
            if visual {
                vim.feed("v").unwrap();
            }
            if vim.feed(&table.prefix.to_string()).is_err() {
                continue;
            }
            let Some(which) = vim.state().which_key() else {
                panic!("nothing after {}", table.prefix);
            };
            let taken = (table.keys.iter())
                .filter(|&&(key, _)| takes(visual, table.prefix, key))
                .count();
            assert_eq!(which.entries.len(), taken, "{} {visual}", table.prefix);
        }
    }
}
