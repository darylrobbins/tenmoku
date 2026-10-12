//! fe-vim's line and screen motions (`ge gE _ + - | H M L gj gk`) against
//! recorded Neovim: `tests/corpus/line_motions.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes. `H M L` run in a
//! window 10 rows high; no row is wider than nvim's window, so `gj gk` move
//! as they do over unwrapped rows.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn line_motions_matches_neovim() {
    let recorded = load("line_motions").unwrap();
    assert!(
        recorded.len() >= 40,
        "line_motions: {} cases",
        recorded.len()
    );
    for case in &recorded {
        if case.keys.contains(['H', 'M', 'L']) {
            assert_eq!(case.window_rows, Some(10), "{}", case.name);
        }
    }
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ge_punct",
        "gE_punct",
        "ge_across_rows",
        "ge_onto_empty_row",
        "ge_at_start_fails",
        "underscore_count",
        "plus_count_clamps",
        "plus_last_row_fails",
        "minus_first_row_fails",
        "bar_past_end",
        "bar_tab",
        "bar_wide",
        "H_count",
        "L_count",
        "M",
        "H_after_ctrl_e",
        "dL",
        "yH",
        "gj",
        "gk",
        "dgj",
        "d_plus_dot",
        // Each motion in charwise visual mode.
        "v_ge",
        "v_gE",
        "v_underscore",
        "v_plus",
        "v_minus",
        "v_bar",
        "v_H_dollar",
        "v_M",
        "v_L",
        "v_gj",
        "v_gk",
        // A visual cursor past the row's end.
        "v_dollar_ge",
        "v_dollar_gE",
        // Counted motions failing part way under an operator, and `y-`.
        "d3ge_empty_first_row",
        "c3ge_empty_first_row",
        "y3ge_empty_first_row",
        "d9gj_past_end",
        "y9gk_past_start",
        "c9gj_past_end",
        "y_minus",
        "y2_minus",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "line_motions: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "line_motions: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
