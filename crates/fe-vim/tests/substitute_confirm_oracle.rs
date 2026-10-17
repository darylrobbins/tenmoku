//! fe-vim's `:s` with the `c` flag (confirm) against recorded Neovim:
//! `tests/corpus/substitute_confirm.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes; and the prompt it shows.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn substitute_confirm_matches_neovim() {
    let recorded = load("substitute_confirm").unwrap();
    assert!(
        recorded.len() >= 16,
        "substitute_confirm: {} cases",
        recorded.len()
    );
    for name in [
        "yes_no_yes",
        "all_after_no",
        "quit_after_yes",
        "last_after_no",
        "esc_then_undo",
        "without_g_one_per_row",
        "across_rows",
        "replacement_longer_than_match",
        "replacement_with_line_break",
        "no_match_fails",
        "amp_amp_keeps_confirm",
        "other_keys_ignored",
        "from_a_macro",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "substitute_confirm: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "substitute_confirm: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// The selection, as rows and byte columns: its anchor and its head.
fn selection(vim: &HeadlessVim) -> ((u32, usize), (u32, usize)) {
    let selection = vim.model().primary_selection();
    let point = |offset: usize| {
        let point = vim.model().text().offset_to_point(offset);
        (point.row, point.column as usize)
    };
    (point(selection.tail()), point(selection.head()))
}

#[test]
fn confirm_prompt_is_nvims() {
    // nvim 0.12's: `:help :s_c` and `usr_10.txt` show it.
    let prompt = "replace with b? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)";
    let mut vim = HeadlessVim::new("aa x aa\naa", (0, 0));
    vim.feed(":%s/aa/b/gc<CR>").unwrap();
    assert_eq!(vim.state().message(), Some(prompt));
    // The match is selected, the caret on its start.
    assert_eq!(selection(&vim), ((0, 2), (0, 0)));
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("y").unwrap();
    assert_eq!(vim.state().message(), Some(prompt));
    assert_eq!(selection(&vim), ((0, 6), (0, 4)));
    // A key the prompt doesn't take leaves it asking.
    vim.feed("x").unwrap();
    assert_eq!(vim.state().message(), Some(prompt));
    assert_eq!(vim.text(), "b x aa\naa");
    vim.feed("y").unwrap();
    assert_eq!(selection(&vim), ((1, 2), (1, 0)));
    vim.feed("y").unwrap();
    assert_eq!(vim.text(), "b x b\nb");
    assert_eq!(vim.state().message(), Some("3 substitutions on 2 lines"));
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(selection(&vim), ((1, 0), (1, 0)));
    // The next key is read as any other: one `u` undoes all three.
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "aa x aa\naa");

    // The replacement as typed, `~` expanded; two substitutions or fewer
    // leave no report, as in nvim.
    let mut vim = HeadlessVim::new("a a a", (0, 0));
    vim.feed(":s/a/x\\r/<CR>u:s/a/[~&]/gc<CR>").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("replace with [x\\r&]? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)")
    );
    vim.feed("yyq").unwrap();
    assert_eq!(vim.text(), "[x\na] [x\na] a");
    assert_eq!(vim.state().message(), None);

    // `a` reports what it replaced, those before it included.
    let mut vim = HeadlessVim::new("a a a a", (0, 0));
    vim.feed(":s/a/b/gc<CR>nya").unwrap();
    assert_eq!(vim.text(), "a b b b");
    assert_eq!(vim.state().message(), Some("3 substitutions on 1 line"));
}
