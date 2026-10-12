//! fe-vim's `'[`, `']`, `'.` and `'^` marks and its uppercase marks against
//! recorded Neovim: `tests/corpus/marks_special.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn marks_special_matches_neovim() {
    let recorded = load("marks_special").unwrap();
    assert!(
        recorded.len() >= 35,
        "marks_special: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "yiw_start",
        "yiw_end",
        "yy_start",
        "yy_end",
        "p_start",
        "p_end",
        "P_start",
        "P_end",
        "dw_start",
        "dd_start",
        "dd_end",
        "cw_start",
        "cw_end",
        "shift_start",
        "shift_end",
        "x_start",
        "visual_d_start",
        "visual_d_end",
        "yy_linewise_end",
        "dj_linewise",
        "change_x",
        "change_after_undo",
        "insert_i",
        "insert_A",
        "upper_quote",
        "upper_backtick",
        "upper_delete_linewise",
        "upper_rows_put_above",
        "insert_rows_put_above",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "marks_special: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "marks_special: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
