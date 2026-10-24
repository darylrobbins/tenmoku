use fe_vim::{Remap, RemapMode};

use super::remap_keymap_conflicts;
use crate::default_keymap;

/// The defaults as on Linux, where insert mode's `ctrl-t` is bound to vim.
fn linux_keymap() -> fe_keymap::Keymap {
    fe_keymap::load_keymap(fe_keymap::DEFAULT_LINUX, None).0
}

fn map(mode: RemapMode, from: &str) -> Remap {
    Remap {
        modes: vec![mode],
        from: from.into(),
        to: "x".into(),
        recursive: false,
    }
}

fn conflicts(mode: RemapMode, from: &str) -> Vec<String> {
    let found = remap_keymap_conflicts(&linux_keymap(), &[map(mode, from)], "\\");
    found
        .iter()
        .map(|c| format!("{} {}", c.keys, c.command))
        .collect()
}

#[test]
fn insert_map_on_a_free_key_has_no_conflict() {
    assert_eq!(conflicts(RemapMode::Insert, "jk"), Vec::<String>::new());
    let maps = [map(RemapMode::Insert, "jk")];
    assert!(remap_keymap_conflicts(&default_keymap(), &maps, "\\").is_empty());
}

#[test]
fn insert_map_on_a_bound_key_conflicts() {
    let found = conflicts(RemapMode::Insert, "<C-t>");
    assert!(!found.is_empty(), "{found:?}");
    assert!(found.iter().all(|c| c.starts_with("ctrl-t ")), "{found:?}");
}

#[test]
fn a_bound_prefix_of_the_left_side_conflicts() {
    let found = conflicts(RemapMode::Insert, "<C-t>x");
    assert!(!found.is_empty(), "{found:?}");
}

#[test]
fn a_longer_binding_than_the_left_side_does_not() {
    let keymap = linux_keymap();
    let maps = [map(RemapMode::Insert, "j")];
    assert!(remap_keymap_conflicts(&keymap, &maps, "\\").is_empty());
}

#[test]
fn cmdline_map_conflicts_with_the_vim_line_bindings() {
    let found = conflicts(RemapMode::CmdLine, "<Esc>");
    assert!(
        found.iter().any(|c| c.ends_with("vim_line.cancel")),
        "{found:?}"
    );
}

#[test]
fn normal_visual_and_operator_maps_raise_none() {
    for mode in [
        RemapMode::Normal,
        RemapMode::Visual,
        RemapMode::OperatorPending,
    ] {
        assert!(conflicts(mode, "<C-t>").is_empty());
    }
}

#[test]
fn the_leader_is_put_in() {
    let keymap = linux_keymap();
    let maps = [map(RemapMode::Insert, "<leader>")];
    assert!(!remap_keymap_conflicts(&keymap, &maps, "<C-t>").is_empty());
    assert!(remap_keymap_conflicts(&keymap, &maps, "\\").is_empty());
}

fn keymap_with(keys: &str, context: &str) -> fe_keymap::Keymap {
    let text = format!(
        "[[bindings]]\nkeys = \"{keys}\"\ncommand = \"test.cmd\"\ncontext = \"{context}\"\n"
    );
    fe_keymap::load_keymap(&text, None).0
}

#[test]
fn a_shifted_symbol_matches_its_shifted_base_key() {
    let keymap = keymap_with("shift-4", "editor && vim.mode == insert");
    let maps = [map(RemapMode::Insert, "$a")];
    assert_eq!(remap_keymap_conflicts(&keymap, &maps, "\\").len(), 1);
}

#[test]
fn leader_notation_is_not_formed_across_the_boundary() {
    let keymap = linux_keymap();
    let maps = [map(RemapMode::Insert, "<leader>C-t>")];
    assert!(remap_keymap_conflicts(&keymap, &maps, "<").is_empty());
}

#[test]
fn a_record_that_cannot_load_has_no_conflict() {
    let keymap = linux_keymap();
    let mut bad = map(RemapMode::Insert, "<C-t>");
    bad.to = "<Bogus>".into();
    assert!(remap_keymap_conflicts(&keymap, &[bad], "\\").is_empty());
    let maps = [map(RemapMode::Insert, "<C-t>")];
    assert!(remap_keymap_conflicts(&keymap, &maps, "<Bogus>").is_empty());
}
