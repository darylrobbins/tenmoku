//! fe-vim's `[<Space>` and `]<Space>` against recorded Neovim:
//! `tests/corpus/line_edits.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes. `[e` and `]e` are vim-unimpaired's,
//! not nvim's: `tests/line_edits.rs` checks them.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn blank_lines_match_neovim() {
    let recorded = load("line_edits").unwrap();
    assert!(recorded.len() >= 30, "line_edits: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "space_below",
        "space_above",
        "space_below_count",
        "space_above_count",
        "space_above_first_row",
        "space_below_last_row",
        "space_below_only_row",
        "space_above_only_row",
        "space_below_empty_buffer",
        "space_above_moves_marks",
        "space_below_moves_marks",
        "space_above_mark_on_empty_row",
        "space_below_dot",
        "space_above_dot_new_count",
        "space_above_undo_redo",
        "space_after_operator_cancels",
        "space_below_in_visual",
        "space_above_in_visual_line",
        "space_below_visual_marks_on_empty_row",
        "space_below_count_visual_marks_on_empty_row",
        "space_above_visual_marks_on_empty_row",
        "space_below_then_above_visual_marks_on_empty_row",
        "space_below_then_above_visual_marks",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "line_edits: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "line_edits: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
