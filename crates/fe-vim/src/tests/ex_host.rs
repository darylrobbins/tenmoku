use crate::{HeadlessVim, HostCommand as H};

fn submit(vim: &mut HeadlessVim, line: &str) {
    vim.feed(&format!(":{line}<CR>")).unwrap();
}

#[test]
fn host_commands_are_handed_over() {
    let cases = [
        (
            "w",
            H::Write {
                force: false,
                path: None,
            },
        ),
        (
            "write",
            H::Write {
                force: false,
                path: None,
            },
        ),
        (
            "w!",
            H::Write {
                force: true,
                path: None,
            },
        ),
        ("q", H::Quit { force: false }),
        ("quit", H::Quit { force: false }),
        ("q!", H::Quit { force: true }),
        ("wq", wq(false, false)),
        ("wq!", wq(true, false)),
        ("x", wq(false, true)),
        ("xit", wq(false, true)),
        ("exi", wq(false, true)),
        ("exit!", wq(true, true)),
        ("e src/main.rs", edit(Some("src/main.rs"), false)),
        ("edit! src/main.rs", edit(Some("src/main.rs"), true)),
        ("e!", edit(None, true)),
        (r"e foo\ bar", edit(Some("foo bar"), false)),
        (r"ed! a\\b\c  ", edit(Some(r"a\bc"), true)),
        (r"e a\\\\b", edit(Some(r"a\\b"), false)),
        (r"e a\\\ b", edit(Some(r"a\ b"), false)),
        (r"edit! a\\ b", edit(Some(r"a\ b"), true)),
        (r"ex a\ \\b", edit(Some(r"a \b"), false)),
        (r"e trailing\ ", edit(Some("trailing "), false)),
        ("e a\\\\ ", edit(Some("a\\ "), false)),
        ("e a\\\\\t", edit(Some("a\\\t"), false)),
        ("ed a\\\\ \t  ", edit(Some("a\\ "), false)),
        ("edit! a\\\\\\\\ ", edit(Some("a\\\\ "), true)),
        ("ex a\\\t ", edit(Some("a\t"), false)),
        ("e", edit(None, false)),
        ("qa", H::QuitAll { force: false }),
        ("qall", H::QuitAll { force: false }),
        ("quita!", H::QuitAll { force: true }),
        ("qa!", H::QuitAll { force: true }),
        ("wa", H::WriteAll),
        ("wall", H::WriteAll),
    ];
    for (line, expected) in cases {
        let mut vim = HeadlessVim::new("one\ntwo", (1, 1));
        submit(&mut vim, line);
        assert_eq!(
            vim.state_mut().take_host_command(),
            Some(expected),
            ":{line}"
        );
        assert_eq!(vim.state_mut().take_host_command(), None, ":{line} again");
        assert_eq!(vim.state().message(), None, ":{line}");
        assert_eq!(
            (vim.text(), vim.cursor(), vim.mode()),
            ("one\ntwo".into(), (1, 1), "normal")
        );
    }
}

#[test]
fn what_a_host_command_doesnt_take_is_an_error() {
    for (line, message) in [
        ("5w", "E481: No range allowed"),
        ("q foo", "E488: Trailing characters: foo"),
        ("wq x", "E488: Trailing characters: x"),
    ] {
        let mut vim = HeadlessVim::new("one", (0, 0));
        submit(&mut vim, line);
        assert_eq!(vim.state().message(), Some(message), ":{line}");
        assert_eq!(vim.state_mut().take_host_command(), None, ":{line}");
    }
}

fn wq(force: bool, only_if_modified: bool) -> H {
    H::WriteQuit {
        force,
        only_if_modified,
    }
}

fn edit(path: Option<&str>, force: bool) -> H {
    H::Edit {
        path: path.map(String::from),
        force,
    }
}

#[test]
fn a_failed_line_drops_an_untaken_command() {
    for line in ["q foo", "5w", "sav", "frobnicate"] {
        let mut vim = HeadlessVim::new("one", (0, 0));
        submit(&mut vim, "q!");
        submit(&mut vim, line);
        assert_eq!(vim.state_mut().take_host_command(), None, ":{line}");
    }
}
