//! `:set number`/`nu` and `:set relativenumber`/`rnu` through
//! [`HeadlessVim`]: the editor's line-number flags each form leaves, and
//! nvim's messages for `opt?`.

use fe_vim::HeadlessVim;

fn run(vim: &mut HeadlessVim, line: &str) -> Option<String> {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    vim.state().message().map(String::from)
}

/// The editor's 'number' and 'relativenumber'.
fn flags(vim: &HeadlessVim) -> (bool, bool) {
    (vim.model().number(), vim.model().relative_number())
}

#[test]
fn ex_set_number_forms() {
    let mut vim = HeadlessVim::new("one\ntwo\nthree", (1, 0));
    // As nvim's defaults, but with 'number' on, as fastedit's editors start.
    assert_eq!(flags(&vim), (true, false));
    assert_eq!(run(&mut vim, "set nu?").as_deref(), Some("  number"));
    assert_eq!(
        run(&mut vim, "set rnu?").as_deref(),
        Some("norelativenumber")
    );

    assert_eq!(run(&mut vim, "set nu rnu"), None);
    assert_eq!(flags(&vim), (true, true));
    assert_eq!(
        run(&mut vim, "set nu? rnu?").as_deref(),
        Some("  number\n  relativenumber")
    );

    run(&mut vim, "set nonu");
    assert_eq!(flags(&vim), (false, true));
    assert_eq!(run(&mut vim, "set nu?").as_deref(), Some("nonumber"));
    assert_eq!(run(&mut vim, "set nonu?").as_deref(), Some("nonumber"));

    run(&mut vim, "set rnu!");
    assert_eq!(flags(&vim), (false, false));
    assert_eq!(
        run(&mut vim, "set rnu?").as_deref(),
        Some("norelativenumber")
    );

    run(&mut vim, "set invnu");
    assert_eq!(flags(&vim), (true, false));
    run(&mut vim, "set invnumber invrelativenumber");
    assert_eq!(flags(&vim), (false, true));
    run(&mut vim, "set number norelativenumber");
    assert_eq!(flags(&vim), (true, false));
    run(&mut vim, "set relativenumber nonumber");
    assert_eq!(flags(&vim), (false, true));
    assert_eq!(
        run(&mut vim, "set relativenumber?").as_deref(),
        Some("  relativenumber")
    );
    run(&mut vim, "set nu! rnu!");
    assert_eq!(flags(&vim), (true, false));

    // A bool takes no value; nothing changes.
    let message = run(&mut vim, "set number=3").unwrap();
    assert!(message.contains("Invalid argument: number=3"), "{message}");
    assert_eq!(flags(&vim), (true, false));
    let message = run(&mut vim, "set rnu=1").unwrap();
    assert!(message.contains("Invalid argument: rnu=1"), "{message}");
    assert_eq!(flags(&vim), (true, false));
    // Applied in turn, up to the failure.
    let message = run(&mut vim, "set rnu nu:0").unwrap();
    assert!(message.contains("Invalid argument: nu:0"), "{message}");
    assert_eq!(flags(&vim), (true, true));

    // The buffer and caret are left alone.
    assert_eq!(vim.text(), "one\ntwo\nthree");
}
