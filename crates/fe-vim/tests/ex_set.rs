//! `:se[t]` through [`HeadlessVim`]: nvim's messages for `opt?`, and the
//! editor state each form leaves.

use fe_editor::IndentStyle;
use fe_vim::HeadlessVim;

fn run(vim: &mut HeadlessVim, line: &str) -> Option<String> {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    vim.state().message().map(String::from)
}

fn vim() -> HeadlessVim {
    HeadlessVim::new("one", (0, 0))
}

#[test]
fn set_reports_options_as_nvim_does() {
    let mut vim = vim();
    // nvim's `wrap` is on; the headless editor starts with soft wrap off.
    vim.model_mut().set_soft_wrap(true);
    assert_eq!(run(&mut vim, "set ts?").as_deref(), Some("  tabstop=4"));
    assert_eq!(run(&mut vim, "set ts").as_deref(), Some("  tabstop=4"));
    assert_eq!(run(&mut vim, "set sw?").as_deref(), Some("  shiftwidth=4"));
    assert_eq!(run(&mut vim, "set et?").as_deref(), Some("  expandtab"));
    assert_eq!(run(&mut vim, "set wrap?").as_deref(), Some("  wrap"));
    assert_eq!(run(&mut vim, "set nowrap?").as_deref(), Some("  wrap"));
    assert_eq!(
        run(&mut vim, "set clipboard?").as_deref(),
        Some("  clipboard=")
    );
    run(&mut vim, "set invwrap");
    assert_eq!(run(&mut vim, "set wrap?").as_deref(), Some("nowrap"));
    assert_eq!(run(&mut vim, "set ts=8 sw=2"), None);
    assert_eq!(run(&mut vim, "set ts?").as_deref(), Some("  tabstop=8"));
    assert_eq!(run(&mut vim, "set sw?").as_deref(), Some("  shiftwidth=2"));
}

#[test]
fn set_changes_the_editor() {
    let mut vim = vim();
    vim.model_mut().set_soft_wrap(true);
    run(&mut vim, "set nowrap");
    assert!(!vim.model().soft_wrap());
    run(&mut vim, "set wrap");
    assert!(vim.model().soft_wrap());
    run(&mut vim, "set wrap!");
    assert!(!vim.model().soft_wrap());
    run(&mut vim, "set invwrap");
    assert!(vim.model().soft_wrap());

    run(&mut vim, "set ts=8");
    assert_eq!(vim.model().tab_size(), 8);
    run(&mut vim, "set tabstop:2");
    assert_eq!(vim.model().tab_size(), 2);
    run(&mut vim, "set sw=3");
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Spaces(3)));
    run(&mut vim, "set sw=0");
    assert_eq!(vim.model().indent_style(), None);
    run(&mut vim, "set ts=8");
    assert_eq!(vim.model().indent_unit(), "        ");
    run(&mut vim, "set ts=2 sw=2");
    run(&mut vim, "set sw=2 noet");
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Tabs));
    run(&mut vim, "set et");
    assert_eq!(vim.model().indent_style(), None);
    run(&mut vim, "set invet");
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Tabs));
    run(&mut vim, "set et!");
    assert_eq!(vim.model().indent_style(), None);
    run(&mut vim, "set sw=3");
    run(&mut vim, "set ts=5");
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Spaces(3)));
    assert_eq!(run(&mut vim, "set sw?").as_deref(), Some("  shiftwidth=3"));
    run(&mut vim, "set ts=2 sw=2");

    run(&mut vim, "set clipboard=unnamedplus");
    assert!(vim.registers().clipboard_unnamed());
    run(&mut vim, "set cb=");
    assert!(!vim.registers().clipboard_unnamed());

    // Several in one, stopping at the first that fails.
    let message = run(&mut vim, "set ts=6 nowrap sw=x ts=3");
    assert_eq!(
        message.as_deref(),
        Some("E521: Number required after =: sw=x")
    );
    assert_eq!(vim.model().tab_size(), 6);
    assert!(!vim.model().soft_wrap());
}

#[test]
fn set_refuses_what_the_editor_cannot_hold() {
    let mut vim = vim();
    run(&mut vim, "set sw=2");
    let before = (vim.model().tab_size(), vim.model().indent_style());
    for line in [
        "set noet",
        "set noexpandtab",
        "set ts&",
        "set ts+=2",
        "set sw-=1",
        "set wrap&",
        "set",
        "set all",
        "set clipboard=unnamed",
        "set foo",
        "set ts=0",
        "set ts=17",
        "set ts=-1",
        "2set ts?",
    ] {
        let message = run(&mut vim, line);
        assert!(message.is_some(), "{line} left no message");
        assert!(vim.state().failed(), "{line} did not fail");
        assert_eq!(
            (vim.model().tab_size(), vim.model().indent_style()),
            before,
            "{line}"
        );
        assert!(!vim.registers().clipboard_unnamed());
    }
    assert_eq!(
        run(&mut vim, "set foo").as_deref(),
        Some("foo: not supported yet")
    );
    assert_eq!(
        run(&mut vim, "set ts=0").as_deref(),
        Some("E487: Argument must be positive: ts=0")
    );
    // Tabs follow tabstop, so tabstop can't change under them.
    run(&mut vim, "set sw=4 noet");
    assert_eq!(
        run(&mut vim, "set ts=8").as_deref(),
        Some("ts=8: not supported under noexpandtab, set expandtab first")
    );
    assert_eq!(vim.model().tab_size(), 4);
    // Tabs under sw=N other than tabstop.
    run(&mut vim, "set sw=4 noet");
    run(&mut vim, "set sw=2");
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Tabs));
}
