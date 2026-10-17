//! fe-vim's `:pu[t][!] [x]` against recorded Neovim: `tests/corpus/ex_put.toml`
//! and its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes; and `:pu =`, which fe-vim refuses.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn ex_put_matches_neovim() {
    let recorded = load("ex_put").unwrap();
    assert!(recorded.len() >= 20, "ex_put: {} cases", recorded.len());
    for name in [
        "put_unnamed_below",
        "put_bang_charwise_above",
        "put_line_zero",
        "put_last_line",
        "put_named_register_with_address",
        "put_numbered_register",
        "put_multiline_register",
        "put_block_register",
        "put_empty_register_fails",
        "put_then_undo",
        "put_then_redo",
        "put_from_global",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "ex_put: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_put: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn put_expression_register_is_refused() {
    for line in [":pu =<CR>", ":pu! =<CR>", ":2pu = 1+1<CR>"] {
        let mut vim = HeadlessVim::new("a\nb", (0, 0));
        vim.feed("yy").unwrap();
        vim.feed(line).unwrap();
        assert_eq!(vim.text(), "a\nb", "{line}");
        assert_eq!(vim.cursor(), (0, 0), "{line}");
        assert!(vim.state().failed(), "{line}");
        let message = vim.state().message().unwrap_or_default();
        assert!(
            message.contains("the = register: not supported"),
            "{line}: {message}"
        );
    }
}
