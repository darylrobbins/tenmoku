//! fe-vim's `]p`, `[p`, `]P` and `[P` against recorded Neovim:
//! `tests/corpus/put_indent.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn put_indent_matches_neovim() {
    let recorded = load("put_indent").unwrap();
    assert!(recorded.len() >= 14, "put_indent: {} cases", recorded.len());
    for name in [
        "bracket_p_below_indents",
        "bracket_p_above_indents",
        "big_p_forms_go_above",
        "outdents_to_a_shallower_row",
        "mixed_indents_keep_their_offsets",
        "first_line_blank",
        "tab_indented_row",
        "charwise_is_plain_put",
        "block_is_plain_put",
        "with_count",
        "then_dot",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "put_indent: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "put_indent: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
