//! fe-vim's `|` between ex commands against recorded Neovim:
//! `tests/corpus/ex_bar.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_bar_matches_neovim() {
    let recorded = load("ex_bar").unwrap();
    assert!(recorded.len() >= 15, "ex_bar: {} cases", recorded.len());
    let names: Vec<&str> = recorded.iter().map(|case| case.name.as_str()).collect();
    for name in [
        "bar_two_substitutes_then_undo",
        "bar_error_stops_the_rest",
        "bar_in_global_command",
        "bar_in_normal_keys",
        "bar_escaped_in_pattern",
        "bar_after_comment",
        "bar_delete_then_yank",
        "bar_range_then_delete",
        "bar_after_host_command_unchanged",
    ] {
        assert!(names.contains(&name), "ex_bar: no case {name}");
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_bar: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
