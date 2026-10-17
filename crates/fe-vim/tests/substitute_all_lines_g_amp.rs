//! Normal-mode `g&`, nvim's `:%s//~/&`: the last substitute again on every
//! line, with the last search pattern and the substitute's flags.

use fe_vim::HeadlessVim;

fn run(buffer: &str, keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(buffer, (0, 0));
    vim.feed(keys).unwrap();
    vim
}

#[test]
fn g_ampersand_substitutes_every_line_as_nvim_does() {
    let text = "a a a\na a a\nA a A";
    assert_eq!(run(text, ":s/a/b/g<CR>g&").text(), "b b b\nb b b\nA b A");
    assert_eq!(
        run("a a a\nx a a\nA a A", ":s/a/b/<CR>/x<CR>g&").text(),
        "b a a\nb a a\nA a A"
    );
    assert_eq!(run(text, ":s/a/b/i<CR>g&").text(), "b b a\nb a a\nb a A");
}

#[test]
fn g_ampersand_is_one_undo_and_not_repeated() {
    let mut vim = run("a\na\na", ":s/a/b/<CR>");
    vim.feed("g&").unwrap();
    assert_eq!(vim.text(), "b\nb\nb");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "b\na\na");
    vim.feed("j.").unwrap();
    assert_eq!(vim.text(), "b\na\na");
}

#[test]
fn g_ampersand_after_an_operator_cancels_and_needs_a_search() {
    let mut vim = run("a\na", ":s/a/b/<CR>");
    vim.feed("dg&").unwrap();
    assert_eq!(vim.text(), "b\na");
    let vim = run("a", "g&");
    assert_eq!(vim.text(), "a");
    assert!(vim.state().message().is_some_and(|m| m.starts_with("E35")));
}
