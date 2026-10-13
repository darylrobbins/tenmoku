//! fe-vim's `:g` following its rows through `gJ` joins against recorded
//! Neovim: `tests/corpus/ex_global_join_raw.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_global_join_raw_matches_neovim() {
    let recorded = load("ex_global_join_raw").unwrap();
    assert!(
        recorded.len() >= 12,
        "ex_global_join_raw: {} cases",
        recorded.len()
    );
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_global_join_raw: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
