//! fe-vim's insert-mode `<C-t>` and `<C-d>` (and `0<C-d>`, `^<C-d>`) against
//! recorded Neovim: `tests/corpus/insert_indent_keys.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn insert_indent_keys_match_neovim() {
    let recorded = load("insert_indent_keys").unwrap();
    assert!(
        recorded.len() >= 12,
        "insert_indent_keys: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_t_indents",
        "ctrl_t_twice",
        "ctrl_d_outdents",
        "ctrl_d_to_previous_stop",
        "ctrl_d_no_indent",
        "zero_ctrl_d",
        "caret_ctrl_d",
        "ctrl_t_dot_repeat",
        "ctrl_t_count_insert",
        "ctrl_d_replace_mode",
        "ctrl_t_replace_mode",
        "replace_zero_ctrl_d",
        "replace_caret_ctrl_d",
        "replace_caret_ctrl_d_enter",
        "counted_o_ctrl_t",
        "counted_O_ctrl_d",
        "caret_ctrl_d_ctrl_t_enter",
        "caret_ctrl_d_ctrl_t_ctrl_d_enter",
        "zero_ctrl_t_ctrl_d",
        "caret_ctrl_t_ctrl_d",
        "ctrl_t_replace_dot",
        "ctrl_d_replace_dot",
        "ctrl_t_replace_count",
        "ctrl_d_replace_count",
        "ctrl_t_replace_dot_count",
        "ctrl_d_after_multibyte",
        "ctrl_d_existing_zero",
        "ctrl_d_caret_inside_indent",
        "literal_private_use_dot",
        "zero_ctrl_d_dot",
        "ctrl_t_dot_count",
        "caret_ctrl_d_enter",
        "ctrl_d_tab_indent",
        "ctrl_d_count_insert",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "insert_indent_keys: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "insert_indent_keys: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
