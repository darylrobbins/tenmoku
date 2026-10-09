//! fe-vim's `/` and `?` (as motions, operator targets and in visual modes)
//! and the `/` register, against recorded Neovim:
//! `tests/corpus/search.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn search_matches_neovim() {
    let recorded = load("search").unwrap();
    assert!(recorded.len() >= 30, "search: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "search: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
