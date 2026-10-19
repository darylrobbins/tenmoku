//! fe-vim's changelist (`g;` and `g,`) and `'.` against recorded Neovim:
//! `tests/corpus/changelist.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn changelist_matches_neovim() {
    let recorded = load("changelist").unwrap();
    assert!(recorded.len() >= 16, "changelist: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "g_semicolon_last_change",
        "g_semicolon_count",
        "g_comma_back",
        "close_changes_merge",
        "insert_session_position",
        "quote_dot",
        "backtick_dot",
        "d_quote_dot",
        "changes_follow_deleted_rows",
        "at_oldest_fails",
        "at_newest_fails",
        "empty_list_fails",
        "visual_g_semicolon",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "changelist: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "changelist: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
