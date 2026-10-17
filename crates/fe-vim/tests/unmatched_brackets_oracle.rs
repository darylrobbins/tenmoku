//! fe-vim's unmatched bracket motions (`[( [{ ]) ]}`) against recorded
//! Neovim: `tests/corpus/unmatched_brackets.toml` and its expectations,
//! which the ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn unmatched_brackets_match_neovim() {
    let recorded = load("unmatched_brackets").unwrap();
    assert!(
        recorded.len() >= 24,
        "unmatched_brackets: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "open_paren",
        "open_paren_count",
        "close_paren",
        "close_paren_nested",
        "close_paren_count",
        "open_brace",
        "open_brace_count",
        "close_brace",
        "close_brace_count",
        "open_paren_across_rows",
        "close_paren_across_rows",
        "open_brace_across_rows",
        "close_brace_across_rows",
        "open_paren_inside_nested",
        "close_paren_inside_nested",
        "open_brace_inside_nested",
        "close_brace_inside_nested",
        "open_paren_skips_pair",
        "close_paren_skips_string",
        "open_paren_skips_string",
        "close_brace_skips_string",
        "open_brace_skips_string",
        "open_paren_none_fails",
        "close_paren_none_fails",
        "open_brace_none_fails",
        "close_brace_none_fails",
        "open_paren_count_past_outermost",
        "open_paren_backtick_back",
        "open_paren_quote_back",
        "close_paren_backtick_back",
        "open_brace_backtick_back",
        "close_brace_backtick_back",
        "v_open_paren_d",
        "v_close_paren_d",
        "v_open_brace_d",
        "v_close_brace_d",
        "v_open_paren_count",
        "v_close_paren_count",
        "V_open_paren_d",
        "V_close_paren_d",
        "V_open_brace_d",
        "V_close_brace_d",
        "v_open_paren_none_fails",
        "v_close_brace_none_fails",
        "v_dollar_open_paren",
        "v_dollar_open_paren_trailing",
        "v_dollar_close_paren_trailing",
        "v_dollar_open_brace_trailing",
        "v_dollar_open_paren_after",
        "v_dollar_close_brace",
        "close_paren_from_opening_quote",
        "close_paren_from_opening_quote_count",
        "close_brace_from_opening_quote",
        "open_paren_from_opening_quote",
        "open_paren_from_closing_quote",
        "close_paren_from_literal_quote",
        "open_paren_from_literal_quote",
        "close_brace_from_literal_quote",
        "open_brace_from_literal_quote",
        "close_paren_skips_literal",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "unmatched_brackets: no case {name}"
        );
    }
    // A motion that fails leaves the jump mark unset, which `check` doesn't
    // compare: ends of the failing cases, fed here.
    for case in recorded.iter().filter(|case| case.name.ends_with("_fails")) {
        let mut vim = HeadlessVim::new(&case.buffer, case.cursor);
        let _ = vim.feed(&case.keys);
        assert_eq!(vim.mark('\''), None, "{}: jump mark set", case.name);
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "unmatched_brackets: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
