//! fe-vim's `g*` and `g#` against recorded Neovim:
//! `tests/corpus/partial_word_search.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn partial_word_search_matches_neovim() {
    let recorded = load("partial_word_search").unwrap();
    assert!(
        recorded.len() >= 12,
        "partial_word_search: {} cases",
        recorded.len()
    );
    for name in [
        "g_star",
        "count_g_star",
        "g_star_n",
        "g_hash",
        "dg_star",
        "g_star_register_put",
        "g_hash_register_put",
        "g_star_keyword_beside_punct",
        "vg_star",
        "g_hash_n",
        "g_star_jump_back",
        "g_star_punct_run",
        "g_star_after_star",
        "g_star_blank_row",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "partial_word_search: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "partial_word_search: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
