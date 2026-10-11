//! fe-vim's replace mode (`R`) against recorded Neovim:
//! `tests/corpus/replace.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn replace_matches_neovim() {
    let recorded = load("replace").unwrap();
    assert!(recorded.len() >= 20, "replace: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "over_row_middle",
        "past_row_end",
        "count_over_row",
        "cr_splits_row",
        "bs_restores",
        "bs_appended",
        "over_multibyte",
        "over_combining",
        "dot",
        "undo",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "replace: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "replace: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
