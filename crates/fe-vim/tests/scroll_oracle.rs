//! fe-vim's scroll keys against recorded Neovim with a window 10 rows high:
//! `tests/corpus/scroll.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn scroll_matches_neovim() {
    let recorded = load("scroll").unwrap();
    assert!(recorded.len() >= 25, "scroll: {} cases", recorded.len());
    for case in &recorded {
        assert_eq!(case.window_rows, Some(10), "{}", case.name);
        assert_eq!(case.buffer.split('\n').count(), 60, "{}", case.name);
        assert!(case.expect.top.is_some(), "{}: no top recorded", case.name);
    }
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_d",
        "ctrl_d_count",
        "ctrl_d_count_persists",
        "ctrl_d_at_end",
        "ctrl_u",
        "ctrl_u_count_persists",
        "ctrl_u_at_top",
        "ctrl_e_count_pushes",
        "ctrl_y_pushes",
        "ctrl_f",
        "ctrl_b",
        "zz",
        "zt",
        "zb",
        "z_cr",
        "z_dot",
        "z_minus",
        "zz_near_top",
        "zt_near_bottom",
        "visual_ctrl_d",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "scroll: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "scroll: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
