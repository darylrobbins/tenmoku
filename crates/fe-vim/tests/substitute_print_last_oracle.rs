//! fe-vim's `:s` print flags (`p`, `#`, `l`) and its `:s\/`, `:s\?` and
//! `:s\&` forms against recorded Neovim:
//! `tests/corpus/substitute_print_last.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn substitute_print_last_matches_neovim() {
    let recorded = load("substitute_print_last").unwrap();
    assert!(
        recorded.len() >= 14,
        "substitute_print_last: {} cases",
        recorded.len()
    );
    for name in [
        "print_flag_p",
        "print_flag_hash",
        "print_flag_l",
        "print_flags_with_g_and_count",
        "amp_amp_keeps_no_print_flag",
        "last_search_slash",
        "last_search_question",
        "last_substitute_amp",
        "last_search_unset_fails",
        "last_substitute_unset_fails",
        "backslash_other_fails",
        "n_after_last_search_form",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "substitute_print_last: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "substitute_print_last: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn an_expression_replacement_stays_refused() {
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed(":s/b/\\=1/<CR>").unwrap();
    assert_eq!(vim.text(), "abc");
    let message = vim.state().message().unwrap_or_default();
    assert!(message.contains("\\="), "{message:?}");
}
