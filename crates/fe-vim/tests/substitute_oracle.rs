//! fe-vim's `:s`, `:&`, `:&&` and normal mode's `&` against recorded
//! Neovim: `tests/corpus/substitute.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn substitute_matches_neovim() {
    let recorded = load("substitute").unwrap();
    assert!(recorded.len() >= 30, "substitute: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "sub_first",
        "sub_global",
        "sub_ignore_case",
        "sub_match_case",
        "sub_keep_flags",
        "sub_count_only",
        "sub_quiet_not_found",
        "empty_pattern_last_search",
        "repeat_bare_s",
        "repeat_ampersand_drops_flags",
        "repeat_ampersand_ampersand_keeps_flags",
        "normal_ampersand",
        "special_whole_match",
        "special_groups",
        "special_tilde",
        "special_line_break",
        "special_tab_and_backslash",
        "special_upper_lower_one",
        "special_upper_lower_run",
        "delimiter_hash",
        "percent_many_rows",
        "visual_line_range",
        "undo_after_percent",
        "not_found",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "substitute: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "substitute: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
