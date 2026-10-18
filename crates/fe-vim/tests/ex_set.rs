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

#[test]
fn set_reads_numbers_in_nvims_bases() {
    let mut vim = vim();
    for (line, tab) in [
        ("set ts=010", 8),
        ("set ts:010", 8),
        ("set ts=017", 15),
        ("set ts=0x8", 8),
        ("set ts=0X8", 8),
        ("set ts=0x08", 8),
        ("set ts=0xa", 10),
        ("set ts=0xA", 10),
        ("set ts=0o10", 8),
        ("set ts=0O10", 8),
        ("set ts=0b101", 5),
        ("set ts=0B11", 3),
        ("set ts=08", 8),
        ("set ts=09", 9),
    ] {
        run(&mut vim, line);
        assert!(!vim.state().failed(), "{line}");
        assert_eq!(vim.model().tab_size(), tab, "{line}");
    }
    run(&mut vim, "set ts=4");
    run(&mut vim, "set sw=0010");
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Spaces(8)));
    assert_eq!(run(&mut vim, "set sw?").as_deref(), Some("  shiftwidth=8"));
    // A leading 0 before an 8 or 9 is decimal.
    for (line, width, query) in [
        ("set sw=018", 18, "  shiftwidth=18"),
        ("set sw=0019", 19, "  shiftwidth=19"),
    ] {
        run(&mut vim, line);
        assert!(!vim.state().failed(), "{line}");
        assert_eq!(vim.model().indent_style(), Some(IndentStyle::Spaces(width)));
        assert_eq!(run(&mut vim, "set sw?").as_deref(), Some(query));
    }
    // ts=018 is decimal 18: over MAX_TABSTOP, refused, tabstop unchanged.
    run(&mut vim, "set ts=4");
    run(&mut vim, "set ts=018");
    assert!(vim.state().failed());
    assert_eq!(vim.model().tab_size(), 4);
    run(&mut vim, "set sw=8");
    assert_eq!(
        run(&mut vim, "set ts=010 ts?").as_deref(),
        Some("  tabstop=8")
    );
    for line in [
        "set sw=00",
        "set sw=0x0",
        "set sw=0b0",
        "set sw=0o0",
        "set sw=-0",
        "set sw=-00",
    ] {
        run(&mut vim, "set sw=3");
        run(&mut vim, line);
        assert!(!vim.state().failed(), "{line}");
        assert_eq!(vim.model().indent_style(), None, "{line}");
    }
    run(&mut vim, "set ts=4 sw=2");
    let before = (vim.model().tab_size(), vim.model().indent_style());
    for arg in [
        "ts=00", "ts=0x0", "ts=-0", "ts=-010", "ts=-0x8", "ts=-08", "sw=-0x1",
    ] {
        let message = run(&mut vim, &format!("set {arg}"));
        assert_eq!(
            message.as_deref(),
            Some(format!("E487: Argument must be positive: {arg}").as_str())
        );
        assert!(vim.state().failed(), "{arg}");
        assert_eq!((vim.model().tab_size(), vim.model().indent_style()), before);
    }
    for arg in [
        "ts=0x", "ts=0X", "ts=0xg", "ts=0x-8", "ts=0b", "ts=0b2", "ts=0b12", "ts=0o", "ts=0o8",
        "ts=0o18", "ts=00x8", "ts=010x", "ts=8x", "ts=0_8", "ts=+8", "ts=1e1",
    ] {
        let message = run(&mut vim, &format!("set {arg}"));
        assert_eq!(
            message.as_deref(),
            Some(format!("E521: Number required after =: {arg}").as_str())
        );
        assert!(vim.state().failed(), "{arg}");
        assert_eq!((vim.model().tab_size(), vim.model().indent_style()), before);
    }
    run(&mut vim, "set ts=4 sw=3");
    let message = run(&mut vim, "set sw=2 ts=0x");
    assert_eq!(
        message.as_deref(),
        Some("E521: Number required after =: ts=0x")
    );
    assert!(vim.state().failed());
    assert_eq!(vim.model().tab_size(), 4);
    assert_eq!(vim.model().indent_style(), Some(IndentStyle::Spaces(2)));
}
