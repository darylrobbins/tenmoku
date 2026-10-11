//! fe-vim's `<C-a>` and `<C-x>` against recorded Neovim:
//! `tests/corpus/increment.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn increment_matches_neovim() {
    let recorded = load("increment").unwrap();
    assert!(recorded.len() >= 25, "increment: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "increment: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
