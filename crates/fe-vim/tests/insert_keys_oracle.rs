//! fe-vim's insert-mode `<C-w>` and `<C-u>` against recorded Neovim:
//! `tests/corpus/insert_keys.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn insert_keys_match_neovim() {
    let recorded = load("insert_keys").unwrap();
    assert!(
        recorded.len() >= 16,
        "insert_keys: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_w_word",
        "ctrl_w_punctuation",
        "ctrl_w_stops_once_at_insert_start",
        "ctrl_w_joins_rows",
        "ctrl_w_in_indent",
        "ctrl_u_typed_text",
        "ctrl_u_nothing_typed",
        "ctrl_u_twice",
        "ctrl_w_dot_repeat",
        "ctrl_u_count_insert",
        "ctrl_w_replace_mode",
        "ctrl_w_in_macro",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "insert_keys: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "insert_keys: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
