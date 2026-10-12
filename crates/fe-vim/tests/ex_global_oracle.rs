//! fe-vim's `:g`, `:v` and `:normal` against recorded Neovim:
//! `tests/corpus/ex_global.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_global_matches_neovim() {
    let recorded = load("ex_global").unwrap();
    assert!(recorded.len() >= 40, "ex_global: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_global: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
