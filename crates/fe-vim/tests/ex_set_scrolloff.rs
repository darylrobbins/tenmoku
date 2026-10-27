//! `:set scrolloff`/`so` through [`HeadlessVim`]: the editor's value each
//! form leaves, nvim's message for `so?`, and the errors.

use fe_vim::HeadlessVim;

fn run(vim: &mut HeadlessVim, line: &str) -> Option<String> {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    vim.state().message().map(String::from)
}

#[test]
fn ex_set_scrolloff_forms() {
    let mut vim = HeadlessVim::new("one\ntwo\nthree", (1, 0));
    // fastedit's editors start with three rows, not nvim's 0.
    assert_eq!(vim.model().scroll_off(), 3);
    assert_eq!(run(&mut vim, "set so?").as_deref(), Some("  scrolloff=3"));

    assert_eq!(run(&mut vim, "set so=5"), None);
    assert_eq!(vim.model().scroll_off(), 5);
    assert_eq!(run(&mut vim, "set so?").as_deref(), Some("  scrolloff=5"));
    assert_eq!(
        run(&mut vim, "set scrolloff?").as_deref(),
        Some("  scrolloff=5")
    );
    assert_eq!(run(&mut vim, "set scrolloff=0"), None);
    assert_eq!(vim.model().scroll_off(), 0);
    assert_eq!(run(&mut vim, "set scrolloff=5"), None);
    assert_eq!(vim.model().scroll_off(), 5);
    assert_eq!(run(&mut vim, "set so:7"), None);
    assert_eq!(vim.model().scroll_off(), 7);
    // The window clamps a huge one, where it scrolls.
    assert_eq!(run(&mut vim, "set so=999"), None);
    assert_eq!(run(&mut vim, "set so?").as_deref(), Some("  scrolloff=999"));
    // Bare `so` shows it too.
    assert_eq!(run(&mut vim, "set so").as_deref(), Some("  scrolloff=999"));
    run(&mut vim, "set so=5");

    let message = run(&mut vim, "set so=-1").unwrap();
    assert!(message.contains("E487"), "{message}");
    let message = run(&mut vim, "set so=x").unwrap();
    assert!(message.contains("E521"), "{message}");
    for arg in ["noso", "invso", "noscrolloff", "so!"] {
        let message = run(&mut vim, &format!("set {arg}")).unwrap();
        assert!(message.contains(arg), "{arg}: {message}");
        assert_eq!(vim.model().scroll_off(), 5, "{arg}");
    }
    assert_eq!(vim.text(), "one\ntwo\nthree");
}
