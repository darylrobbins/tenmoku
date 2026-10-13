//! fe-vim's `gJ` against recorded Neovim: `tests/corpus/join_raw.toml` and
//! its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn join_raw_matches_neovim() {
    let recorded = load("join_raw").unwrap();
    assert!(recorded.len() >= 20, "join_raw: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "gJ_two_rows",
        "gJ_count_3",
        "gJ_count_past_end",
        "gJ_last_row",
        "gJ_count_3_last_row",
        "gJ_indented_next",
        "gJ_paren",
        "v_gJ_three_rows",
        "V_gJ_three_rows",
        "ctrl_v_gJ_three_rows",
        "dot_after_gJ",
        "dot_after_V_gJ",
        "u_after_gJ",
        "dgJ_cancels",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "join_raw: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "join_raw: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
