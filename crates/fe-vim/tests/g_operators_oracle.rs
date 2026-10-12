//! fe-vim's g operators (`gu gU g~ g? gq gw`) against recorded Neovim:
//! `tests/corpus/g_operators.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn g_operators_matches_neovim() {
    let recorded = load("g_operators").unwrap();
    assert!(
        recorded.len() >= 45,
        "g_operators: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "gugu",
        "guu",
        "tilde_g_tilde",
        "tilde_tilde",
        "count_rot13_g_rot13",
        "rot13_rot13",
        "v_gu",
        "V_gU",
        "block_rot13",
        "non_ascii_gU",
        "dot_gU_w",
        "undo_gU_w",
        "gqq_long",
        "gqj_short",
        "gqG_paragraphs",
        "gww_long",
        "gqq_indented",
        "gqq_word_too_long",
        "dot_gqq",
        "undo_gqq",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "g_operators: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "g_operators: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
