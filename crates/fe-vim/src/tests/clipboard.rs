//! `clipboard=unnamedplus` in the registers, by `:help clipboard-unnamedplus`
//! and `nvim --clean -c 'set clipboard=unnamedplus'` with pbcopy by hand (the
//! oracle has no clipboard provider): yanks and deletes that name no register
//! write the clipboard, puts that name none read it.

use crate::HeadlessVim;

fn vim(text: &str, unnamed: bool) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.set_clipboard_unnamed(unnamed);
    vim
}

fn clip(vim: &HeadlessVim) -> Option<&str> {
    vim.host().clipboard.as_deref()
}

fn reg(vim: &HeadlessVim, name: char) -> Option<String> {
    vim.registers().get(name).map(|r| r.text.clone())
}

#[test]
fn unnamedplus() {
    let mut v = vim("one two\nthree", true);
    v.feed("yy").unwrap();
    assert_eq!(clip(&v), Some("one two\n"));
    assert_eq!(reg(&v, '"').as_deref(), Some("one two\n"));
    assert_eq!(reg(&v, '0').as_deref(), Some("one two\n"));

    v.feed("dw").unwrap();
    assert_eq!(clip(&v), Some("one "));
    assert_eq!(reg(&v, '-').as_deref(), Some("one "));

    v.feed("\"ayy").unwrap();
    assert_eq!(clip(&v), Some("one "));
    v.feed("\"_dd").unwrap();
    assert_eq!(clip(&v), Some("one "));

    // A put with no register reads the clipboard: linewise when it ends in
    // a line break.
    let mut v = vim("x", true);
    v.host_mut().clipboard = Some("a\n".into());
    v.feed("p").unwrap();
    assert_eq!(v.text(), "x\na");

    let mut v = vim("one\ntwo", false);
    v.feed("yy").unwrap();
    assert_eq!(clip(&v), None);
    v.host_mut().clipboard = Some("keep".into());
    v.feed("jyy").unwrap();
    assert_eq!(clip(&v), Some("keep"));
}

#[test]
fn kinds_round_trip() {
    // A charwise yank ending in a line break stays charwise.
    let mut v = vim("ab\ncd", false);
    v.feed("v$\"+y").unwrap();
    assert_eq!(clip(&v), Some("ab\n"));
    v.feed("G0\"+p").unwrap();
    assert_eq!(v.text(), "ab\ncab\nd");

    // A block of width 2 pastes as a block.
    let mut v = vim("abc\ndef", false);
    v.feed("<C-v>jl\"+y").unwrap();
    assert_eq!(clip(&v), Some("ab\nde"));
    v.feed("$\"+p").unwrap();
    assert_eq!(v.text(), "abcab\ndefde");

    // Text another writer left is linewise when it ends in a line break.
    let mut v = vim("x", false);
    v.host_mut().clipboard = Some("l\n".into());
    v.feed("\"+p").unwrap();
    assert_eq!(v.text(), "x\nl");

    // An empty clipboard puts nothing.
    let mut v = vim("x", false);
    v.host_mut().clipboard = Some(String::new());
    v.feed("\"+p").unwrap();
    assert_eq!(v.text(), "x");
}
