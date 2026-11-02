//! `u` and `<C-r>` swapping fe-vim's `a` to `z` and last visual extent with
//! those from before the change, against recorded Neovim:
//! `tests/corpus/undo_marks.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn undo_marks_matches_neovim() {
    let recorded = load("undo_marks").unwrap();
    // What the card asks the corpus to cover, by case name.
    for name in [
        "dd_undo",
        "dd_undo_redo",
        "ex_d_undo",
        "join_carries_then_undo",
        "join_carried_mark_undo",
        "dG_undo",
        "visual_then_dG_undo",
        "global_d_undo",
        "two_undos_over_two_changes",
        "cc_on_the_marked_row_undo",
        "bracket_e_up_undo",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "undo_marks: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "undo_marks: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

const ROWS: &str = "one\ntwo\nthree\nfour";

/// A filter's output arrives outside any command, and headless nvim's own
/// shell isn't the oracle's, so these were probed by hand (`nvim --clean
/// --headless`, `:2,3!head -1` and `:0r !echo x`, `getpos()`).
#[test]
fn a_filter_puts_back_the_mark_on_a_row_it_dropped() {
    let mut vim = HeadlessVim::new(ROWS, (1, 0));
    vim.feed("majmb").unwrap();
    let mut state = std::mem::take(vim.state_mut());
    state.apply_filter(vim.model_mut(), 1..3, "two\n");
    *vim.state_mut() = state;
    assert_eq!(vim.text(), "one\ntwo\nfour");
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((1, 0)), None));
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), ROWS);
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((1, 0)), Some((2, 0))));
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.text(), "one\ntwo\nfour");
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((1, 0)), None));
}

#[test]
fn a_read_command_puts_back_a_mark_deleted_after_it() {
    let mut vim = HeadlessVim::new(ROWS, (1, 0));
    vim.feed("ma").unwrap();
    let mut state = std::mem::take(vim.state_mut());
    state.apply_read_command(vim.model_mut(), 0, "x\n");
    *vim.state_mut() = state;
    assert_eq!(vim.mark('a'), Some((2, 0)));
    vim.feed(":delmarks a<CR>").unwrap();
    assert_eq!(vim.mark('a'), None);
    vim.feed("u").unwrap();
    assert_eq!(
        (vim.text(), vim.mark('a')),
        (ROWS.to_string(), Some((1, 0)))
    );
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.mark('a'), Some((2, 0)));
}
