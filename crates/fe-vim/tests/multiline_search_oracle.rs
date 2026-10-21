//! fe-vim's searches whose pattern matches across rows (`/c\nd`, `?`, `n`,
//! `N`, counts, offsets, an operator or a visual selection over them)
//! against recorded Neovim: `tests/corpus/multiline_search.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn multiline_search_matches_neovim() {
    let recorded = load("multiline_search").unwrap();
    assert!(
        recorded.len() >= 14,
        "multiline_search: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "slash_across_rows",
        "question_across_rows",
        "n_next_across",
        "N_back_across",
        "count_across",
        "e_offset_across",
        "line_offset_across",
        "d_slash_across",
        "visual_search_across",
        "wrap_across",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "multiline_search: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "multiline_search: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
