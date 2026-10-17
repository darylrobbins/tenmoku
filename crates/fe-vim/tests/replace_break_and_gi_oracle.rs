//! `r<CR>` and `gi` against recorded Neovim: `tests/corpus/replace_break_and_gi.toml`
//! and its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn replace_break_and_gi_match_neovim() {
    let recorded = load("replace_break_and_gi").unwrap();
    assert!(
        recorded.len() >= 20,
        "replace_break_and_gi: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "r_cr",
        "r_ctrl_j",
        "r_register",
        "r_blank_after_dropped",
        "r_blank_before_kept",
        "r_indented",
        "r_tab_indented",
        "r_count",
        "r_last_char",
        "r_dot",
        "r_undo",
        "r_count_too_few",
        "gi_no_insert_yet",
        "gi_after_A",
        "gi_count",
        "gi_after_O",
        "gi_after_rows_put_above",
        "gi_after_cw",
        "gi_after_R",
        "gi_dot",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "replace_break_and_gi: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "replace_break_and_gi: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
