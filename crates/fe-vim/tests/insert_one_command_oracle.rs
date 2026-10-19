//! fe-vim's insert-mode `<C-o>` against recorded Neovim:
//! `tests/corpus/insert_one_command.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn insert_one_command_matches_neovim() {
    let recorded = load("insert_one_command").unwrap();
    assert!(
        recorded.len() >= 14,
        "insert_one_command: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_o_motion",
        "ctrl_o_at_row_end",
        "ctrl_o_delete",
        "ctrl_o_count",
        "ctrl_o_register_put",
        "ctrl_o_visual_returns",
        "ctrl_o_command_line_returns",
        "ctrl_o_escape_returns",
        "ctrl_o_open_row",
        "ctrl_o_dot_after",
        "ctrl_o_undo",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "insert_one_command: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "insert_one_command: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
