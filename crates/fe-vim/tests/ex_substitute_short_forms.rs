//! `:s`'s two-letter forms, `:~` and the `r` flag, as nvim --clean takes them.

use fe_vim::HeadlessVim;

fn run(text: &str, cursor: (u32, usize), lines: &[&str]) -> String {
    let mut vim = HeadlessVim::new(text, cursor);
    for line in lines {
        vim.feed(&format!(":{line}<CR>")).unwrap();
        let message = vim.state().message().map(String::from);
        assert!(
            !message.as_deref().is_some_and(|m| m.starts_with('E')),
            ":{line}: {message:?}"
        );
    }
    vim.text()
}

#[test]
fn two_letter_substitute_forms_repeat_with_their_flags() {
    let text = "a a a\na a a\nA a A";
    let go = |lines: &[&str]| run(text, (0, 0), lines);
    assert_eq!(
        go(&["s/a/b/", "2", "sg", "3", "s/a/c/", "si"]),
        "b a a\nb b b\nc c A"
    );
    assert_eq!(go(&["s/a/b/g", "2", "sI"]), "b b b\nb a a\nA a A");
    assert_eq!(go(&["s/a/b/", "2", "sg 2"]), "b a a\nb b b\nA b A");

    let text = "a a a\nx a x\nA a A";
    for line in ["&r", "sr", "s r"] {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed(":s/a/b/g<CR>/x<CR>").unwrap();
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(vim.state().message(), None, ":{line}");
        assert_eq!(vim.text(), "b b b\nb a x\nA a A", ":{line}");
    }
    for line in ["~g", "&&r"] {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed(":s/a/b/g<CR>/x<CR>").unwrap();
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(vim.state().message(), None, ":{line}");
        assert_eq!(vim.text(), "b b b\nb a b\nA a A", ":{line}");
    }
}
