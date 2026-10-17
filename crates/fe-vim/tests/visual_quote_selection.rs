//! A quote object typed over a charwise selection of more than one position
//! selects or extends as nvim's `current_quote` does. Each expectation was
//! probed with `nvim --clean --headless` (0.12.6, the oracle's settings).

use fe_vim::HeadlessVim;

/// Feeds `keys` and checks the selection is `anchor..cursor` (columns,
/// inclusive, either way round): the cursor, the mode, and the text a yank
/// then puts in the unnamed register.
fn check(buffer: &str, col: usize, keys: &str, anchor: usize, cursor: usize) {
    let mut vim = HeadlessVim::new(buffer, (0, col));
    vim.feed(keys).unwrap();
    let what = format!("{keys:?} on {buffer:?} at {col}");
    assert_eq!(vim.mode(), "visual", "{what}");
    assert_eq!(vim.cursor(), (0, cursor), "{what}");
    vim.feed("y").unwrap();
    let (low, high) = (anchor.min(cursor), anchor.max(cursor));
    let end = (high + 1).min(buffer.len());
    let register = vim.registers().get('"').unwrap().text.clone();
    assert_eq!(register, &buffer[low..end], "{what}");
}

#[test]
fn quotes_over_a_selection_match_nvim() {
    let foo = "foo \"bar baz\" qux";
    check(foo, 6, "vli\"", 5, 11);
    check(foo, 2, "vllli\"", 2, 11);
    check(foo, 6, "vi\"a\"", 4, 13);
    check(foo, 7, "vhi\"", 11, 5);
    check(foo, 6, "vlla\"", 4, 13);
    check(foo, 15, "vhhhhhi\"", 15, 5);
    check("x 'b c' d", 3, "vli'", 3, 5);
    check("x 'b c' d", 3, "vla'", 3, 7);
    check("\"ab\" x \"cd\"", 1, "vi\"i\"i\"", 0, 9);
    check("\"ab\" x \"cd\"", 1, "vlllli\"", 1, 9);
    check(foo, 5, "vlli\"", 5, 11);
}

#[test]
fn quotes_nvim_already_matched_stay() {
    let foo = "foo \"bar baz\" qux";
    check(foo, 7, "vi\"", 5, 11);
    check(foo, 7, "vi\"i\"", 4, 12);
    check(foo, 6, "vlllllli\"", 6, 12);
    check(foo, 9, "vllli\"", 9, 12);
    check(foo, 10, "vhhhhhhhi\"", 10, 3);
}

#[test]
fn backward_quotes_reaching_the_buffer_start_do_not_panic() {
    check("\"abc\"", 2, "vhhi\"", 2, 1);
    check("'abc'", 2, "vhhi'", 2, 1);
}
