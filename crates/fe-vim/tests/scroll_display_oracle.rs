//! fe-vim's scroll keys against recorded Neovim under closed folds and soft
//! wrap, a window 10 rows high: `tests/corpus/scroll_display.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn scroll_display_matches_neovim() {
    let recorded = load("scroll_display").unwrap();
    assert!(
        recorded.len() >= 23,
        "scroll_display: {} cases",
        recorded.len()
    );
    for case in &recorded {
        assert_eq!(case.window_rows, Some(10), "{}", case.name);
        assert_eq!(case.buffer.split('\n').count(), 60, "{}", case.name);
        assert!(
            !case.folds.is_empty() || case.soft_wrap,
            "{}: neither folds nor soft wrap",
            case.name
        );
        assert!(case.expect.top.is_some(), "{}: no top recorded", case.name);
    }
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_d_fold",
        "ctrl_d_twice_fold",
        "ctrl_d_count_fold",
        "ctrl_u_fold",
        "ctrl_e_fold",
        "ctrl_e_count_fold",
        "ctrl_y_fold",
        "ctrl_f_fold",
        "ctrl_b_fold",
        "zt_fold",
        "zz_fold",
        "zb_fold",
        "visual_ctrl_d_fold",
        "ctrl_d_wrap",
        "ctrl_d_twice_wrap",
        "ctrl_u_wrap",
        "ctrl_e_wrap",
        "ctrl_e_twice_wrap",
        "ctrl_y_wrap",
        "zt_wrap",
        "zz_wrap",
        "zb_wrap",
        "zb_wrap_partial",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "scroll_display: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "scroll_display: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
