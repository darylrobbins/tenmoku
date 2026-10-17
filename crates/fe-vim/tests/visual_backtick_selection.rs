//! Backtick objects typed over a charwise selection of more than one position
//! select or extend as the quote objects do. Each expectation was probed with
//! `nvim --clean --headless` (0.12.6, the oracle's settings).

use fe_vim::HeadlessVim;

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
fn backtick_objects_over_a_charwise_selection_match_nvim() {
    let two = "a `bc` d `ef` g";
    check(two, 0, "vli`", 3, 4);
    check(two, 0, "vlla`", 0, 9);
    check(two, 4, "vla`", 4, 13);
    check(two, 9, "vhi`", 9, 3);
    let foo = "x `foo bar` y";
    check(foo, 4, "vli`", 3, 9);
    check(foo, 4, "vli`a`", 2, 11);
    check("x \\`a` `b`", 6, "vli`", 6, 8);
    // already matching
    check(foo, 5, "vi`", 3, 9);
    check(foo, 5, "vi`i`", 2, 10);

    let mut vim = HeadlessVim::new(foo, (0, 4));
    vim.feed("vlli`d").unwrap();
    assert_eq!(vim.text(), "x `` y");
    assert_eq!(vim.cursor(), (0, 3));
}
