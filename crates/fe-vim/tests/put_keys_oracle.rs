//! fe-vim's `Y`, `gp` and `gP` against recorded Neovim:
//! `tests/corpus/put_keys.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn put_keys_match_neovim() {
    let recorded = load("put_keys").unwrap();
    assert!(recorded.len() >= 18, "put_keys: {} cases", recorded.len());
    for name in [
        "y_to_line_end",
        "y_with_count",
        "y_into_register",
        "y_in_visual_is_linewise",
        "gp_linewise",
        "gp_linewise_count",
        "gp_linewise_at_buffer_end",
        "gp_charwise",
        "gP_linewise",
        "gP_charwise",
        "gp_block",
        "gp_then_dot",
        "gp_empty_register_fails",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "put_keys: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "put_keys: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn put_args_without_cursor_after_serialize_as_before() {
    let plain = fe_vim::PutArgs {
        before: false,
        cursor_after: false,
        indent: false,
        count: None,
        register: None,
    };
    assert_eq!(
        serde_json::to_string(&plain).unwrap(),
        r#"{"before":false}"#
    );
    let after = fe_vim::PutArgs {
        cursor_after: true,
        ..plain
    };
    assert_eq!(
        serde_json::to_string(&after).unwrap(),
        r#"{"before":false,"cursor_after":true}"#
    );
}
