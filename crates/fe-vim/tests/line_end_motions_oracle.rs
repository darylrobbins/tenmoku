//! fe-vim's `g_`, `g$` and `gm` against recorded Neovim:
//! `tests/corpus/line_end_motions.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes. The window is nvim's 80
//! columns, which the headless host reports as the view's columns; the long
//! rows are soft-wrapped at 80.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn line_end_motions_match_neovim() {
    let recorded = load("line_end_motions").unwrap();
    assert!(
        recorded.len() >= 24,
        "line_end_motions: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        // `g_` with counts, under `d`, in visual mode, failing past the end.
        "g_",
        "g_count",
        "d2g_",
        "dg_",
        "d3g_one_row",
        "vg_d",
        "g_past_last_row",
        // `g$` on a short row, then `j`, and on the soft-wrapped long row.
        "g_dollar_short",
        "g_dollar_long_0",
        "g_dollar_long_100",
        "g_dollar_long_count",
        // `gm` on a short row and on the long row.
        "gm_short",
        "gm_long_0",
        "gm_long_100",
        // Each in charwise visual mode.
        "v_g_",
        "v_g_dollar",
        "v_gm",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "line_end_motions: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "line_end_motions: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
