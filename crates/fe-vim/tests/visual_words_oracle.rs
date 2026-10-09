//! fe-vim's iw and iW in visual mode at row ends and blank rows,
//! against recorded Neovim: `tests/corpus/visual_words.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn visual_words_matches_neovim() {
    let recorded = load("visual_words").unwrap();
    assert!(
        recorded.len() >= 40,
        "visual_words: {} cases",
        recorded.len()
    );
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "visual_words: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
