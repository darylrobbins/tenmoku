//! fe-vim's `U` against recorded Neovim: `tests/corpus/line_undo.toml` and
//! its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes. The harness loads each buffer as a change on its first row, so
//! the cases change rows from the second on; a case with no U line to find
//! has nvim clear it first (`oracle_keys`).

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn line_undo_matches_neovim() {
    let recorded = load("line_undo").unwrap();
    assert!(recorded.len() >= 12, "line_undo: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "U_after_x",
        "U_after_several",
        "UU_redoes",
        "U_then_u",
        "U_after_insert",
        "U_after_other_row_change",
        "U_with_nothing_fails",
        "U_after_count_x",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "line_undo: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "line_undo: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
