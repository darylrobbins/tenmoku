//! fe-vim's jumplist (`<C-o>`, `<C-i>` and `<Tab>`) against recorded
//! Neovim: `tests/corpus/jumplist.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn jumplist_matches_neovim() {
    let recorded = load("jumplist").unwrap();
    assert!(recorded.len() >= 30, "jumplist: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "back_twice_then_tab",
        "back_twice_then_ctrl_i",
        "count_back",
        "count_forward",
        "searches",
        "percent_rows",
        "mark_jump",
        "same_row_twice",
        "dd_entry_row",
        "put_in_above",
        "jump_mark_after_back",
        "back_past_oldest",
        "forward_past_newest",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "jumplist: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "jumplist: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
