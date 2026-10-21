//! fe-vim's 'ignorecase' and 'smartcase' for `:s`, `:g`, `:v`, `:sort /pat/`
//! and `/pat/` addresses against recorded Neovim:
//! `tests/corpus/ex_search_case.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes, each case's `set` run
//! before its keys.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_search_case_matches_neovim() {
    let recorded = load("ex_search_case").unwrap();
    assert!(
        recorded.len() >= 12,
        "ex_search_case: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "s_ic",
        "s_scs_uppercase",
        "s_flag_upper_i_wins",
        "s_flag_i_wins",
        "s_empty_pattern",
        "amp_repeat",
        "g_ic",
        "v_scs",
        "sort_pattern_ic",
        "address_pattern_ic",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "ex_search_case: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_search_case: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
