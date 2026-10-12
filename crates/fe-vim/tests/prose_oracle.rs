//! fe-vim's `{ } ( )` and `ip ap is as` against recorded Neovim:
//! `tests/corpus/prose.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn prose_matches_neovim() {
    let recorded = load("prose").unwrap();
    assert!(recorded.len() >= 40, "prose: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "prose: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
