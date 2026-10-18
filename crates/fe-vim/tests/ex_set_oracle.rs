//! fe-vim's `:set` (`wrap`, `tabstop`, `shiftwidth`, `expandtab`,
//! `clipboard`) against recorded Neovim: `tests/corpus/ex_set.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_set_matches_neovim() {
    let recorded = load("ex_set").unwrap();
    assert!(recorded.len() >= 10, "ex_set: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_set: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
