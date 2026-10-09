//! fe-vim's `n`, `N`, `*` and `#` (as motions, operator targets and in visual
//! modes) against recorded Neovim: `tests/corpus/search_next.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn search_next_matches_neovim() {
    let recorded = load("search_next").unwrap();
    assert!(
        recorded.len() >= 20,
        "search_next: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "n_after_slash",
        "n_count_after_slash",
        "n_wraps",
        "N_after_slash",
        "NN_after_slash",
        "Nn_after_slash",
        "n_after_question",
        "n_count_after_question",
        "n_wraps_after_question",
        "N_after_question",
        "NN_after_question",
        "N_count_after_question",
        "N_wraps_after_question",
        "star_on_word",
        "hash_on_word",
        "star_on_punctuation",
        "star_between_words",
        "hash_between_words",
        "star_no_keyword_blanks",
        "dn",
        "d_star",
        "n_visual",
        "star_latin_then_cjk",
        "hash_latin_then_cjk",
        "slash_invalid_then_NN",
        "question_invalid_then_NN",
        "spot_row_22",
        "spot_row_23",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "search_next: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "search_next: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
