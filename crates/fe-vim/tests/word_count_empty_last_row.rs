//! A counted `w` or `W` under an operator that runs out onto the text's
//! empty last row stops at the end of the row before, as in nvim. Each
//! expectation was probed with `nvim --clean --headless`.

use fe_vim::HeadlessVim;

/// Feeds `keys` on `buffer` at `cursor` and checks the text, the cursor and
/// the unnamed register's text and linewise flag.
fn check(buffer: &str, cursor: (u32, usize), keys: &str, want: (&str, (u32, usize), &str, bool)) {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    let register = vim.registers().get('"').unwrap();
    assert_eq!(
        (
            vim.text(),
            vim.cursor(),
            register.text.clone(),
            register.linewise
        ),
        (want.0.to_string(), want.1, want.2.to_string(), want.3),
        "{keys:?} on {buffer:?} at {cursor:?}"
    );
}

#[test]
fn counted_word_running_out_stops_before_the_empty_last_row() {
    check("foo bar\n", (0, 4), "d2w", ("foo \n", (0, 3), "bar", false));
    let vim = {
        let mut vim = HeadlessVim::new("foo bar\n", (0, 4));
        vim.feed("d2w").unwrap();
        vim
    };
    assert!(vim.registers().get('-').is_some_and(|r| r.text == "bar"));
    assert!(vim.registers().get('1').is_none_or(|r| r.text.is_empty()));
    check(
        "foo bar\n",
        (0, 4),
        "y2w",
        ("foo bar\n", (0, 4), "bar", false),
    );
    check("x foo\n", (0, 2), "d9w", ("x \n", (0, 1), "foo", false));
    check("foo bar\n", (0, 0), "d3w", ("", (0, 0), "foo bar\n", true));
    check("foo\n", (0, 0), "d2W", ("", (0, 0), "foo\n", true));
    check("foo\n\n", (0, 0), "d9w", ("", (0, 0), "foo\n\n", true));
    check("foo\n\n", (1, 0), "d2w", ("foo\n", (1, 0), "\n", true));
    check("foo\n  \n", (0, 2), "d2w", ("fo\n", (0, 1), "o\n  ", false));
    check("foo\n  \n", (1, 1), "d2w", ("foo\n", (1, 0), "  \n", true));
    check(
        "  foo\n",
        (0, 0),
        "c9wX<Esc>",
        ("  X\n", (0, 2), "  foo\n", true),
    );
    check(
        "foo\n\n",
        (1, 0),
        "c2wX<Esc>",
        ("foo\nX\n", (1, 0), "\n", true),
    );
    // The count doesn't run out.
    check("foo\n", (0, 0), "dw", ("\n", (0, 0), "foo", false));
    check("foo bar\n", (0, 0), "d2w", ("\n", (0, 0), "foo bar", false));
    check("foo\n\n", (0, 0), "d2w", ("", (0, 0), "foo\n\n", true));
}
