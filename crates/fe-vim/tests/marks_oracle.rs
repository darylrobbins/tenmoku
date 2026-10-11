//! fe-vim's marks (`m`, `'` and `` ` ``, `''`, `'<` and `'>`, and marks in
//! ex ranges) against recorded Neovim: `tests/corpus/marks.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn marks_match_neovim() {
    let recorded = load("marks").unwrap();
    assert!(recorded.len() >= 25, "marks: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "quote_a",
        "backtick_a",
        "delete_quote_a_up",
        "delete_backtick_a",
        "yank_quote_a",
        "visual_quote_a",
        "visual_backtick_a",
        "quote_quote_after_G",
        "backtick_backtick_after_G",
        "quote_quote_after_search",
        "backtick_backtick_after_search",
        "quote_quote_after_percent",
        "backtick_backtick_after_percent",
        "quote_quote_after_colon_5",
        "backtick_backtick_after_colon_5_alone",
        "deleted_row",
        "inserted_above",
        "quote_lt_after_visual",
        "backtick_gt_after_visual",
        "ex_range_marks",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "marks: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "marks: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
