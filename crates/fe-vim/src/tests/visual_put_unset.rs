use crate::{HeadlessVim, Register, Write};

const TEXT: &str = "abc\ndef\nghi";

/// Feeds `keys` over `TEXT`, giving its text and cursor.
fn run(keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

/// Counted `p` and `P` over each selection put nothing, as the uncounted
/// ones do, with `register` selecting the register.
fn counted_puts_nothing(register: &str) {
    for (select, want) in [
        ("ggV", "def\nghi"),
        ("ggjV", "abc\nghi"),
        ("GV", "abc\ndef"),
        ("ggVG", ""),
    ] {
        for put in ["p", "P"] {
            let plain = run(&format!("{select}{register}{put}"));
            let counted = run(&format!("{select}{register}2{put}"));
            assert_eq!(counted.0, want, "{select}{register}2{put}");
            assert_eq!(counted, plain, "{select}{register}2{put}");
        }
    }
}

#[test]
fn a_counted_put_over_lines_from_the_unset_unnamed_register_puts_nothing() {
    counted_puts_nothing("");
}

#[test]
fn a_counted_put_over_lines_from_an_unset_named_register_puts_nothing() {
    counted_puts_nothing("\"a");
}

#[test]
fn a_huge_count_from_an_unset_register_is_not_refused() {
    assert_eq!(run("ggV99999999p").0, "def\nghi");
}

#[test]
fn an_empty_characterwise_register_still_puts_a_row_per_copy() {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    let host = &mut fe_editor::commands::HeadlessHost::new();
    vim.registers_mut()
        .write(Some('a'), Write::Yank, Register::new("", false), host)
        .unwrap();
    vim.feed("ggjV2\"ap").unwrap();
    assert_eq!(vim.text(), "abc\n\n\nghi");
}
