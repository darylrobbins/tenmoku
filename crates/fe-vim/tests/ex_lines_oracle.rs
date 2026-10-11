//! fe-vim's ex line commands (`:y`, `:j`, `:m`, `:t` and `:co`, `:>` and
//! `:<`, `:sort`) against recorded Neovim: `tests/corpus/ex_lines.toml` and
//! its expectations, which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load};

#[test]
fn ex_lines_matches_neovim() {
    let recorded = load("ex_lines").unwrap();
    assert!(recorded.len() >= 45, "ex_lines: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_lines: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
