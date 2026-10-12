//! fe-vim's search offsets (`/foo/e+1`, `/foo/s-1`, `/foo/+2`, `?foo?e`), as
//! motions, operator targets and in visual mode, kept by `n` and `N`, against
//! recorded Neovim: `tests/corpus/search_offset.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn search_offset_matches_neovim() {
    let recorded = load("search_offset").unwrap();
    assert!(
        recorded.len() >= 35,
        "search_offset: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "e",
        "e_n",
        "e_N",
        "e_plus",
        "e_plus_n",
        "e_plus_N",
        "e_minus",
        "e_minus_n",
        "e_minus_N",
        "s_plus",
        "s_plus_n",
        "s_plus_N",
        "b_minus",
        "b_minus_n",
        "b_minus_N",
        "lines_plus",
        "lines_plus_n",
        "lines_plus_N",
        "lines_minus_bare",
        "lines_minus_n",
        "lines_minus_N",
        "question_e",
        "question_e_n",
        "question_e_N",
        "e_plus_crosses_a_row_end",
        "e_plus_crossing_n",
        "e_plus_crossing_N",
        "s_minus_crossing_n",
        "s_minus_crossing_N",
        "slash_slash_e",
        "slash_slash_e_n",
        "d_e",
        "d_lines",
        "c_s",
        "v_e",
        "v_e_n",
        "d_e_dot",
        "star_after_e",
        "star_after_e_n",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "search_offset: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "search_offset: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
