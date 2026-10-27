//! fe-vim's `]]` `[[` `][` `[]` against recorded Neovim:
//! `tests/corpus/sections.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes. Each test replays the cases whose
//! names start with its prefix.

#[allow(dead_code)]
mod support;

use support::{check, load};

/// Replays the cases named `prefix*`, which must hold each of `names`.
fn matches_neovim(prefix: &str, at_least: usize, names: &[&str]) {
    let recorded: Vec<_> = load("sections")
        .unwrap()
        .into_iter()
        .filter(|case| case.name.starts_with(prefix))
        .collect();
    assert!(
        recorded.len() >= at_least,
        "sections {prefix}: {} cases",
        recorded.len()
    );
    for name in names {
        assert!(
            recorded.iter().any(|case| case.name == *name),
            "sections: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "sections {prefix}: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn sections_match_neovim() {
    matches_neovim(
        "plain_",
        40,
        &[
            "plain_next",
            "plain_back",
            "plain_close_next",
            "plain_close_back",
            "plain_count",
            "plain_count_back",
            "plain_past_last",
            "plain_past_first",
            "plain_from_open_row",
            "plain_from_open_row_back",
            "plain_formfeed",
            "plain_macros",
            "plain_not_macros",
            "plain_empty_buffer",
            "plain_python",
            "plain_goal_reset",
        ],
    );
}

#[test]
fn sections_after_operators_match_neovim() {
    matches_neovim(
        "op_",
        25,
        &[
            "op_d_next",
            "op_d_inside",
            "op_d_to_last_row",
            "op_d_count_with_close_stops",
            "op_d_close_on_last_row",
            "op_d_open_on_last_row",
            "op_y_back",
            "op_c_close_next",
            "op_c_next_dot",
            "op_d_dot",
            "op_d_dot_twice",
        ],
    );
}

#[test]
fn sections_visual_and_jumps_match_neovim() {
    matches_neovim(
        "visual_",
        10,
        &[
            "visual_next",
            "visual_back",
            "visual_linewise",
            "visual_inside",
            "visual_count_fails",
        ],
    );
    matches_neovim(
        "jump_",
        10,
        &[
            "jump_back_exact",
            "jump_after_operator",
            "jump_failed_count_keeps",
            "jump_failed_count_back_keeps",
            "jump_moved_in_normal",
            "jump_failed_count_past_end",
        ],
    );
}

#[test]
fn sections_without_braces_fail_quietly() {
    matches_neovim(
        "none_",
        10,
        &[
            "none_count_fails",
            "none_count_close_fails",
            "none_count_keeps_jump",
            "none_operator_fails",
            "none_macro_stops",
            "none_macro_count_stops",
            "none_visual_fails",
        ],
    );
}
