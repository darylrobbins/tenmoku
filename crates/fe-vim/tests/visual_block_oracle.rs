//! fe-vim's blockwise visual mode, and `p` and `P` in each visual mode,
//! against recorded Neovim: `tests/corpus/visual_block.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn visual_block_matches_neovim() {
    let recorded = load("visual_block").unwrap();
    assert!(
        recorded.len() >= 35,
        "visual_block: {} cases",
        recorded.len()
    );
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "visual_block: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
