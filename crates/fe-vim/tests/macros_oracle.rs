//! fe-vim's macros (`q`, `@`, `@@`) against recorded Neovim:
//! `tests/corpus/macros.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes. A macro's register is compared as
//! every register is, its text and kind.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn macros_match_neovim() {
    let recorded = load("macros").unwrap();
    assert!(recorded.len() >= 25, "macros: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "play_count",
        "at_at",
        "append_qA",
        "insert_esc",
        "insert_cr_play",
        "search_in_macro",
        "ex_substitute_in_macro",
        "motion_fails_stops",
        "put_recorded",
        "yank_then_play",
        "dot_after_play",
        "recursive_yank_fails",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "macros: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "macros: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
