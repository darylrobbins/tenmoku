//! `:set` for 'ignorecase', 'smartcase' and 'incsearch' through
//! [`HeadlessVim`]: nvim's messages for each form, the values on the shared
//! registers, and the effect on a following `/`.

use fe_vim::HeadlessVim;

fn run(vim: &mut HeadlessVim, line: &str) -> Option<String> {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    vim.state().message().map(String::from)
}

#[test]
fn set_search_options_report_as_nvim_does() {
    let mut vim = HeadlessVim::new("x\nHello\nhello", (0, 0));
    assert_eq!(run(&mut vim, "set ic?").as_deref(), Some("noignorecase"));
    assert_eq!(run(&mut vim, "set scs?").as_deref(), Some("nosmartcase"));
    assert_eq!(run(&mut vim, "set is?").as_deref(), Some("  incsearch"));
    assert_eq!(
        run(&mut vim, "set incsearch?").as_deref(),
        Some("  incsearch")
    );

    assert_eq!(run(&mut vim, "set ic"), None);
    assert_eq!(vim.registers().search_case(), (true, false));
    assert_eq!(run(&mut vim, "set ic?").as_deref(), Some("  ignorecase"));
    assert_eq!(run(&mut vim, "set noic"), None);
    assert_eq!(vim.registers().search_case(), (false, false));
    assert_eq!(run(&mut vim, "set noic?").as_deref(), Some("noignorecase"));

    run(&mut vim, "set invscs");
    assert_eq!(vim.registers().search_case(), (false, true));
    run(&mut vim, "set invscs");
    assert_eq!(vim.registers().search_case(), (false, false));

    run(&mut vim, "set is!");
    assert!(!vim.registers().incsearch());
    assert_eq!(run(&mut vim, "set is?").as_deref(), Some("noincsearch"));
    run(&mut vim, "set is!");
    assert!(vim.registers().incsearch());
    run(&mut vim, "set noincsearch");
    assert!(!vim.registers().incsearch());
    run(&mut vim, "set invis");
    assert!(vim.registers().incsearch());

    assert_eq!(run(&mut vim, "set ignorecase smartcase"), None);
    assert_eq!(vim.registers().search_case(), (true, true));
    assert_eq!(
        run(&mut vim, "set ic? scs?").as_deref(),
        Some("  ignorecase\n  smartcase")
    );

    // Refused as the other bools' `&` and value forms are, changing nothing.
    assert!(run(&mut vim, "set ic&").is_some_and(|m| m.contains("not supported")));
    assert!(run(&mut vim, "set ic=1").is_some_and(|m| m.contains("E475")));
    assert!(run(&mut vim, "set noic=1").is_some());
    assert_eq!(vim.registers().search_case(), (true, true));
    assert!(vim.registers().incsearch());
}

#[test]
fn set_search_case_changes_a_following_search() {
    let mut vim = HeadlessVim::new("x\nHello\nhello", (0, 0));
    run(&mut vim, "set ic");
    vim.feed("/hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    run(&mut vim, "set scs");
    vim.feed("gg/hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    vim.feed("gg/Hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    run(&mut vim, "set noic");
    vim.feed("gg/hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
}

#[test]
fn set_smartcase_decides_where_an_uppercase_search_lands() {
    let mut vim = HeadlessVim::new("x\nhello\nHello", (0, 0));
    run(&mut vim, "set ic");
    vim.feed("/Hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    run(&mut vim, "set scs");
    vim.feed("gg/Hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
    run(&mut vim, "set noscs");
    vim.feed("gg/Hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    run(&mut vim, "set invscs");
    vim.feed("gg/Hello<CR>").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
}
