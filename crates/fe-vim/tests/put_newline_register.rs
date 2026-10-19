//! `p` and `P` of a charwise text that begins with a line break: from an
//! ordinary register the caret goes where the text starts, as for any
//! multi-row text; from `.`, which nvim puts as if typed again, it goes to
//! the first non-blank of the row after the break. The expectations are
//! nvim 0.12's.

use fe_editor::commands::HeadlessHost;
use fe_vim::{HeadlessVim, Register, Write};

/// `keys` on `abc\nz` at (0, 0), register `a` holding `\n x` charwise.
fn put_from_a(keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new("abc\nz", (0, 0));
    vim.registers_mut()
        .write(
            Some('a'),
            Write::Yank,
            Register::new("\n x", false),
            &mut HeadlessHost::new(),
        )
        .unwrap();
    vim.feed(keys).unwrap();
    vim
}

#[test]
fn an_ordinary_register_leaves_the_caret_where_the_text_starts() {
    let after = put_from_a("\"ap");
    assert_eq!(after.text(), "a\n xbc\nz");
    assert_eq!(after.cursor(), (0, 0));
    let before = put_from_a("\"aP");
    assert_eq!(before.text(), "\n xabc\nz");
    assert_eq!(before.cursor(), (0, 0));
}

#[test]
fn the_last_insert_goes_to_the_row_after_the_break() {
    for (keys, text) in [("\".p", "a\n xbc\nz"), ("\".P", "\n xabc\nz")] {
        let mut vim = HeadlessVim::new("abc\nz", (0, 0));
        vim.feed("A<CR> x<Esc>ugg0").unwrap();
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), text, "{keys}");
        assert_eq!(vim.cursor(), (1, 1), "{keys}");
    }
}
