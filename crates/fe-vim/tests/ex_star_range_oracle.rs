//! fe-vim's `*` ex range, the last visual area, against recorded Neovim:
//! `tests/corpus/ex_star_range.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use fe_vim::Ex;
use support::{check, load};

#[test]
fn ex_star_range_matches_neovim() {
    let recorded = load("ex_star_range").unwrap();
    assert!(
        recorded.len() >= 8,
        "ex_star_range: {} cases",
        recorded.len()
    );
    let names: Vec<&str> = recorded.iter().map(|case| case.name.as_str()).collect();
    for name in [
        "star_delete",
        "star_substitute",
        "star_alone_goes_to_last",
        "star_with_offset",
        "star_before_any_visual_fails",
        "star_shift",
    ] {
        assert!(names.contains(&name), "ex_star_range: no case {name}");
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_star_range: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn star_parses_as_the_visual_marks() {
    assert_eq!(
        Ex::parse("*d").unwrap().range,
        Ex::parse("'<,'>d").unwrap().range
    );
}
