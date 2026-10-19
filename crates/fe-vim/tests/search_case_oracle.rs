//! fe-vim's 'ignorecase' and 'smartcase' for `/ ? n N * #` against recorded
//! Neovim: `tests/corpus/search_case.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes, each case's `set` run
//! before its keys. And the highlight a search leaves, which matches as the
//! search did.

#[allow(dead_code)]
mod support;

use fe_vim::{HeadlessVim, search};
use support::{check, load};

#[test]
fn search_case_matches_neovim() {
    let recorded = load("search_case").unwrap();
    assert!(
        recorded.len() >= 18,
        "search_case: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ic_slash",
        "ic_question",
        "ic_n_and_N",
        "scs_lowercase_ignores",
        "scs_uppercase_exact",
        "scs_backslash_class",
        "lower_c_overrides",
        "upper_c_overrides",
        "star_ic",
        "star_ignores_scs",
        "hash_ic",
        "n_after_star",
        "default_exact",
        "ic_operator_search",
        "ic_visual_search",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "search_case: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "search_case: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// The rows the highlight matches on, each match's start column.
fn highlighted(vim: &HeadlessVim) -> Vec<(u32, u32)> {
    let text = vim.model().text();
    let pattern = vim.state().highlight().expect("a highlight");
    search::matches_in(text, 0..text.row_count(), pattern, 100)
        .into_iter()
        .map(|found| {
            let point = text.offset_to_point(found.start);
            (point.row, point.column)
        })
        .collect()
}

#[test]
fn search_case_highlight_and_register() {
    let mut vim = HeadlessVim::new("x\nHello world\nhello\nHELLO", (0, 0));
    vim.set_search_case(true, true);
    assert_eq!(vim.registers().search_case(), (true, true));

    // Lowercase: every case of it.
    vim.feed("/hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(highlighted(&vim), [(1, 0), (2, 0), (3, 0)]);
    assert_eq!(vim.registers().get('/').unwrap().text, "hello");

    // An uppercase letter: exactly.
    vim.feed("/Hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(highlighted(&vim), [(1, 0)]);
    assert_eq!(vim.registers().get('/').unwrap().text, "Hello");

    // `\c` overrides 'smartcase'; the register keeps it as typed.
    vim.feed("/\\cHello<CR>").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
    assert_eq!(highlighted(&vim), [(1, 0), (2, 0), (3, 0)]);
    assert_eq!(vim.registers().get('/').unwrap().text, "\\cHello");

    // `*` takes 'ignorecase' alone, and so does `n` after it.
    vim.feed("gg0j*").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
    assert_eq!(highlighted(&vim), [(1, 0), (2, 0), (3, 0)]);
    assert_eq!(vim.registers().get('/').unwrap().text, "\\<Hello\\>");
    vim.feed("n").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    assert_eq!(highlighted(&vim), [(1, 0), (2, 0), (3, 0)]);

    // 'ignorecase' off: `n` matches exactly, from the options as they are
    // now, not when the search was typed.
    vim.feed("/hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    vim.set_search_case(false, true);
    vim.feed("n").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
    assert_eq!(highlighted(&vim), [(2, 0)]);
}

#[test]
fn unknown_set_option_fails_the_check() {
    let recorded = load("search_case").unwrap();
    let mut case = recorded
        .into_iter()
        .find(|case| case.name == "ic_slash")
        .unwrap();
    assert_eq!(check(&case), Ok(()));
    case.set = Some("ignorecase hlsearch".to_string());
    let error = check(&case).unwrap_err();
    assert!(error.contains("hlsearch"), "{error}");
    case.set = Some("noignorecase nosuch".to_string());
    let error = check(&case).unwrap_err();
    assert!(error.contains("nosuch"), "{error}");
}
