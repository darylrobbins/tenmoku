//! fe-vim's delimiter objects `i[ a[ i< a< i` a` it at`, against recorded
//! Neovim: `tests/corpus/delimiters.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn delimiters_matches_neovim() {
    let recorded = load("delimiters").unwrap();
    assert!(recorded.len() >= 40, "delimiters: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "delimiters: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
