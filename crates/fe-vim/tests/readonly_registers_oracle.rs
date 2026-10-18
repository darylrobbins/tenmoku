//! fe-vim's read-only registers `.` (the last inserted text) and `:` (the
//! last command line) against recorded Neovim:
//! `tests/corpus/readonly_registers.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes. The cases put the registers
//! into the buffer; a yank, delete or change naming one fails.

#[allow(dead_code)]
mod support;

use fe_vim::{HeadlessVim, Register, Registers};
use support::{check, load};

#[test]
fn readonly_registers_match_neovim() {
    let recorded = load("readonly_registers").unwrap();
    assert!(
        recorded.len() >= 14,
        "readonly_registers: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "dot_after_insert",
        "dot_after_later_delete",
        "dot_after_dot_repeat",
        "dot_after_replace_mode",
        "dot_with_count",
        "dot_in_visual_put",
        "colon_after_substitute",
        "colon_after_failed_command",
        "colon_not_set_by_cancelled_line",
        "colon_not_set_by_search",
        "dot_unset_fails",
        "yank_into_dot_fails",
        "delete_into_colon_fails",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "readonly_registers: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "readonly_registers: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn get_returns_the_read_only_registers_charwise() {
    let mut vim = HeadlessVim::new("abc", (0, 0));
    assert_eq!(vim.registers().get('.'), None);
    assert_eq!(vim.registers().get(':'), None);
    vim.feed("ahi<Esc>:s/b/B/<CR>").unwrap();
    assert_eq!(vim.registers().get('.'), Some(&Register::new("hi", false)));
    assert_eq!(
        vim.registers().get(':'),
        Some(&Register::new("s/b/B/", false))
    );
    assert!(Registers::check('.').is_err());
    assert!(Registers::check(':').is_err());
}
