//! fe-vim's `:u[ndo]` and `:red[o]` against recorded Neovim:
//! `tests/corpus/ex_undo.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes; and `:undo {N}`, which fe-vim
//! refuses.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn ex_undo_matches_neovim() {
    let recorded = load("ex_undo").unwrap();
    assert!(recorded.len() >= 12, "ex_undo: {} cases", recorded.len());
    for name in [
        "undo_once",
        "undo_abbreviated_u",
        "redo_after_undo",
        "redo_abbreviated_red",
        "undo_zero_undoes_all",
        "undo_with_nothing_to_undo_fails",
        "undo_range_fails",
        "undo_bang_fails",
        "dot_after_undo",
        "undo_from_global",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "ex_undo: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_undo: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn undo_to_a_change_number_is_refused() {
    for (line, number) in [
        (":undo 1<CR>", "1"),
        (":u 2<CR>", "2"),
        (":1undo<CR>", "1"),
        (":3u<CR>", "3"),
        (":undo 18446744073709551615<CR>", "18446744073709551615"),
        (":undo 99999999999999999999999<CR>", "18446744073709551615"),
        (
            ":0,0undo 99999999999999999999999<CR>",
            "18446744073709551615",
        ),
    ] {
        let mut vim = HeadlessVim::new("abc\ndef\nghi", (0, 0));
        vim.feed("xjxjx").unwrap();
        let (text, cursor) = (vim.text(), vim.cursor());
        vim.feed(line).unwrap();
        assert_eq!(vim.text(), text, "{line}");
        assert_eq!(vim.cursor(), cursor, "{line}");
        assert!(vim.state().failed(), "{line}");
        let message = vim.state().message().unwrap_or_default();
        assert!(
            message.contains(&format!(":undo {number}: not supported")),
            "{line}: {message}"
        );
    }
}

#[test]
fn undo_with_an_invalid_range_changes_nothing() {
    for line in [":-5undo<CR>", ":1,0undo<CR>", ":9undo<CR>"] {
        let mut vim = HeadlessVim::new("abc\ndef\nghi", (0, 0));
        vim.feed("xjxjx").unwrap();
        let (text, cursor) = (vim.text(), vim.cursor());
        vim.feed(line).unwrap();
        assert_eq!(vim.text(), text, "{line}");
        assert_eq!(vim.cursor(), cursor, "{line}");
        assert!(vim.state().failed(), "{line}");
    }
}
