//! fe-vim's `gs` and `gS` against recorded Neovim: `tests/corpus/jump.toml`
//! and its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes. nvim has no jump, so each case's `oracle_keys` swap it for a mark
//! motion (or a search) to the same place; mark `z` and register `/` are
//! what only the swap leaves, so they're dropped from nvim's side.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn jump_matches_neovim() {
    let recorded = load("jump").unwrap();
    assert!(recorded.len() >= 30, "jump: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| {
            assert!(case.oracle_keys.is_some(), "{}: no oracle_keys", case.name);
            let mut case = case.clone();
            case.expect.marks.remove("z");
            case.expect.registers.remove("/");
            check(&case).err()
        })
        .collect();
    assert!(
        failures.is_empty(),
        "jump: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
