//! An operator over `[(` `[{` `])` `]}` against recorded Neovim:
//! `tests/corpus/unmatched_bracket_operators.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn unmatched_bracket_operators_match_neovim() {
    let recorded = load("unmatched_bracket_operators").unwrap();
    assert!(
        recorded.len() >= 10,
        "unmatched_bracket_operators: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "d_back_paren",
        "d_forward_paren",
        "y_forward_paren",
        "c_forward_paren",
        "d2_back_paren",
        "d_back_paren_none",
        "d_back_brace",
        "d_forward_brace",
        "d_forward_paren_dot",
        "d_bracket_c_cancels",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "unmatched_bracket_operators: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "unmatched_bracket_operators: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
