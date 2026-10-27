//! `:set cursorline`/`cul` through [`HeadlessVim`]: the editor's flag each
//! form leaves, nvim's messages, and the errors.

use fe_vim::HeadlessVim;

fn run(vim: &mut HeadlessVim, line: &str) -> Option<String> {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    vim.state().message().map(String::from)
}

#[test]
fn ex_set_cursorline_forms() {
    let mut vim = HeadlessVim::new("one\ntwo", (1, 0));
    // fastedit's editors band the caret's row, not nvim's off.
    assert!(vim.model().cursor_line());
    assert_eq!(run(&mut vim, "set cul?").as_deref(), Some("  cursorline"));

    assert_eq!(run(&mut vim, "set nocul"), None);
    assert!(!vim.model().cursor_line());
    assert_eq!(run(&mut vim, "set cul?").as_deref(), Some("nocursorline"));
    assert_eq!(run(&mut vim, "set cul"), None);
    assert!(vim.model().cursor_line());
    assert_eq!(run(&mut vim, "set invcul"), None);
    assert!(!vim.model().cursor_line());
    assert_eq!(run(&mut vim, "set cul!"), None);
    assert!(vim.model().cursor_line());
    assert_eq!(run(&mut vim, "set nocursorline"), None);
    assert!(!vim.model().cursor_line());
    assert_eq!(
        run(&mut vim, "set cursorline?").as_deref(),
        Some("nocursorline")
    );
    // Bare `cul` sets it.
    assert_eq!(run(&mut vim, "set cul"), None);
    assert!(vim.model().cursor_line());

    let message = run(&mut vim, "set cul=1").unwrap();
    assert!(message.contains("cul=1"), "{message}");
    assert!(vim.model().cursor_line());
    assert_eq!(vim.text(), "one\ntwo");
}
