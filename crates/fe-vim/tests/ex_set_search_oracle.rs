//! fe-vim's `:set` for 'ignorecase', 'smartcase' and 'incsearch' against
//! recorded Neovim: `tests/corpus/ex_set_search.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_set_search_matches_neovim() {
    let recorded = load("ex_set_search").unwrap();
    assert!(
        recorded.len() >= 8,
        "ex_set_search: {} cases",
        recorded.len()
    );
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_set_search: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
