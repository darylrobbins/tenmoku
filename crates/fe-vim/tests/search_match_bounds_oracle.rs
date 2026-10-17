//! `\zs` and `\ze` in search and `:s` patterns against recorded Neovim:
//! `tests/corpus/search_match_bounds.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn search_match_bounds_match_neovim() {
    let recorded = load("search_match_bounds").unwrap();
    assert!(
        recorded.len() >= 20,
        "search_match_bounds: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "slash_zs",
        "slash_zs_n",
        "question_zs",
        "slash_ze",
        "slash_ze_n",
        "slash_zs_past_cursor",
        "slash_zs_N",
        "d_slash_zs",
        "sub_zs",
        "sub_ze",
        "sub_overlap_zs",
        "sub_zs_leading",
        "sub_empty_zs_g",
        "sub_group_beside_zs",
        "sub_amp_zs",
        "slash_zs_branch",
        "slash_zs_repeated_group",
        "sub_zs_branch",
        "slash_zs_very_magic",
        "sub_zs_very_magic",
        "sub_zs_and_ze",
        "slash_zs_second_row",
        "slash_zs_backtrack",
        "sub_zs_backtrack",
        "slash_zs_not_found",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "search_match_bounds: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "search_match_bounds: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
