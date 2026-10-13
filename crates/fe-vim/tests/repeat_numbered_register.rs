//! `.` after a change made with a numbered register `1`-`8` named takes the
//! next register, as in nvim (`:help redo-register`): not only for a put but
//! for a delete or change too. Each expectation was probed with
//! `nvim --clean --headless` (0.12.6).

use fe_vim::HeadlessVim;

/// The non-empty registers among `" 0-9 -`, as (name, text).
fn registers(vim: &HeadlessVim) -> Vec<(char, String)> {
    "\"0123456789-"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            (!register.text.is_empty()).then(|| (name, register.text.clone()))
        })
        .collect()
}

/// Feeds `keys` to a vim on `buffer` at `cursor` and checks the text and the
/// registers.
fn check(buffer: &str, cursor: (u32, usize), keys: &str, text: &str, want: &[(char, &str)]) {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    let want: Vec<(char, String)> = want
        .iter()
        .map(|(name, text)| (*name, text.to_string()))
        .collect();
    assert_eq!(
        (vim.text(), registers(&vim)),
        (text.to_string(), want),
        "{keys:?} on {buffer:?}"
    );
}

const ROWS: &str = "a\nb\nc\nd\ne\nf";

#[test]
fn dot_after_a_numbered_register_change_takes_the_next_register() {
    check(
        "abcdef",
        (0, 0),
        "\"1x..",
        "def",
        &[('"', "c"), ('1', "a"), ('2', "b"), ('3', "c")],
    );
    check(
        "abcdef",
        (0, 5),
        "\"1X.",
        "abcf",
        &[('"', "d"), ('1', "e"), ('2', "d")],
    );
    check(
        "a b c d",
        (0, 0),
        "\"1dw.",
        "c d",
        &[('"', "b "), ('1', "a "), ('2', "b ")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1Dj.",
        "\n\nc\nd\ne\nf",
        &[('"', "b"), ('1', "a"), ('2', "b")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1dd.",
        "c\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n"), ('3', "b\n")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1dd2.",
        "d\ne\nf",
        &[
            ('"', "b\nc\n"),
            ('1', "b\nc\n"),
            ('2', "a\n"),
            ('3', "b\nc\n"),
        ],
    );
    check(
        "ab cd ef",
        (0, 0),
        "\"1cwx<Esc>w.",
        "x x ef",
        &[('"', "cd"), ('1', "ab"), ('2', "cd")],
    );
    check(
        "abcdef",
        (0, 0),
        "\"1sx<Esc>l.",
        "xxcdef",
        &[('"', "b"), ('1', "a"), ('2', "b")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1ccx<Esc>j.",
        "x\nx\nc\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n"), ('3', "b\n")],
    );
    // Over a selection too.
    check(
        "abcdef",
        (0, 0),
        "vl\"1d.",
        "ef",
        &[('"', "cd"), ('1', "ab"), ('2', "cd")],
    );
    check(
        ROWS,
        (0, 0),
        "V\"1d.",
        "c\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n"), ('3', "b\n")],
    );
    // `8` goes to `9`, and `9` stays.
    check(
        ROWS,
        (0, 0),
        "\"8dd..",
        "d\ne\nf",
        &[('"', "c\n"), ('1', "c\n"), ('2', "b\n"), ('3', "a\n")],
    );
    check(
        ROWS,
        (0, 0),
        "\"9dd.",
        "c\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n")],
    );
}
