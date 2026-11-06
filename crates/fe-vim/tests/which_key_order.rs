//! Which-key's order (`WhichKey::sort_entries`): the keys that run
//! something first, then the groups, each by key, case aside, a lowercase
//! one before its uppercase; for vim's own lists and for a keymap chord's
//! merged with vim's, as fe-gpui builds it.

use std::sync::Arc;

use fe_vim::{
    HeadlessVim, Remap, RemapMode, RemapSet, WhichKey, WhichKeyEntry, WhichKeyLabel, parse_keys,
};

fn map(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

fn vim(remaps: &[Remap]) -> HeadlessVim {
    let mut vim = HeadlessVim::new("one two\n", (0, 0));
    let (set, issues) = RemapSet::new(remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

fn listed(which: &WhichKey) -> Vec<&str> {
    (which.entries.iter())
        .map(|entry| entry.key.as_str())
        .collect()
}

/// Every item lists before every group.
fn items_first(which: &WhichKey) -> bool {
    let first_group = which.entries.iter().position(|entry| entry.group);
    let last_item = which.entries.iter().rposition(|entry| !entry.group);
    match (first_group, last_item) {
        (Some(group), Some(item)) => item < group,
        _ => true,
    }
}

#[test]
fn items_come_before_groups() {
    let remaps = [
        map("<leader>ff", "x"),
        map("<leader>fr", "x"),
        map("<leader>bd", "x"),
        map("<leader>bb", "x"),
        map("<leader>,", "x"),
    ];
    let mut vim = vim(&remaps);
    vim.feed("\\").unwrap();
    let which = vim.state().which_key().unwrap();
    assert_eq!(listed(&which), [",", "b", "f"]);
    assert!(!which.entries[0].group);
    assert!(which.entries[1].group && which.entries[2].group);
    // The same for a host holding the keys itself.
    let held = vim
        .state()
        .which_key_for(&parse_keys("\\").unwrap())
        .unwrap();
    assert_eq!(held, which);
}

/// fe-gpui's `g` chord list: vim's `g` keys, then the keymap's
/// continuations (`g b`, `g d` commands, `g r` a group of five chords),
/// a chord's key winning; sorted, the group `r` comes after every item.
#[test]
fn keymap_chords_order_items_before_groups() {
    let vim = vim(&[]);
    let mut which = vim
        .state()
        .which_key_for(&parse_keys("g").unwrap())
        .unwrap();
    assert!(which.entries.iter().all(|entry| !entry.group));
    let chords = [
        (
            "b",
            WhichKeyLabel::Command("editor.add_caret".into()),
            false,
        ),
        ("d", WhichKeyLabel::Command("lsp.definition".into()), false),
        ("r", WhichKeyLabel::Group(5), true),
    ];
    for (key, label, group) in chords {
        which.entries.retain(|entry| entry.key != key);
        which.entries.push(WhichKeyEntry {
            key: key.into(),
            label,
            group,
        });
    }
    which.sort_entries();
    assert!(items_first(&which), "{:?}", listed(&which));
    let keys = listed(&which);
    assert_eq!(keys.last(), Some(&"r"));
    let at = |key| keys.iter().position(|listed| *listed == key).unwrap();
    // The items stay in key order, a chord's among vim's.
    assert!(at("b") < at("c") && at("c") < at("d") && at("d") < at("e"));
    assert!(at("e") + 1 == at("E"));
}
