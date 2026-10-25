//! The window commands as the host receives them: `:sp`, `:vs`, `:new`,
//! `:vne`, `:on` and `:clo`. The windows are the host's, which the nvim
//! oracle can't see, so the facts are stated here, from nvim 0.12.4
//! (`nvim --clean --headless`):
//!
//! - `fullcommand()`: `sp`, `spl` are `split`; `vs`, `vsp` `vsplit`; `new`
//!   `new` (`n` and `ne` are `next`); `vne` `vnew` (`vn` is `vnoremap`);
//!   `on` `only` (`o` is `oldfiles`); `clo` `close` (`cl` is `clist`); and
//!   `sv` is `sview`, `sf` `sfind`, which fe-vim doesn't run.
//! - `:sp!`, `:vs!`, `:new!` and `:vne!` split as without the `!`; `:3sp`
//!   and `:3vs` split, the count the new window's size; `:sp x"y` opens
//!   `x`, the rest a comment; `:new x` and `:vnew x` open `x`.
//! - `:on` with one window says "Already only one window"; `:clo` and
//!   `:clo!` there are "E444: Cannot close last window".
//! - `:only x` and `:close x` are "E488: Trailing characters: x"; `:on 2`
//!   and `:close 2` take the window number as a count.

use fe_vim::{Ex, ExCommand, ExError, HeadlessVim, HostCommand, WindowCommand};

fn parse(line: &str) -> Result<ExCommand, ExError> {
    Ex::parse(line).map(|ex| ex.command)
}

fn host(line: &str) -> WindowCommand {
    match parse(line) {
        Ok(ExCommand::Host(HostCommand::Window(command))) => command,
        other => panic!(":{line} gave {other:?}"),
    }
}

fn split(vertical: bool, file: Option<&str>) -> WindowCommand {
    WindowCommand::Split {
        vertical,
        file: file.map(String::from),
    }
}

#[test]
fn split_and_vsplit() {
    for line in ["sp", "spl", "spli", "split", "sp!", "3sp", "sp  ", "sp \"x"] {
        assert_eq!(host(line), split(false, None), ":{line}");
    }
    for line in [
        "vs", "vsp", "vspl", "vspli", "vsplit", "vs!", "3vs", "vs \"x",
    ] {
        assert_eq!(host(line), split(true, None), ":{line}");
    }
    // A count is the new window's height or width, which panes don't
    // take: accepted and ignored.
    for line in [
        "sp other.rs",
        "split other.rs  ",
        "2sp other.rs",
        "sp! other.rs",
    ] {
        assert_eq!(host(line), split(false, Some("other.rs")), ":{line}");
    }
    assert_eq!(host("vs other.rs"), split(true, Some("other.rs")));
    assert_eq!(host("sp x\"y"), split(false, Some("x")));
    assert_eq!(host("sp a\\ b.rs"), split(false, Some("a b.rs")));
}

#[test]
fn new_and_vnew() {
    for line in ["new", "new!", "3new", "new \"x"] {
        assert_eq!(
            host(line),
            WindowCommand::New { vertical: false },
            ":{line}"
        );
    }
    for line in ["vne", "vnew", "vne!", "3vne"] {
        assert_eq!(host(line), WindowCommand::New { vertical: true }, ":{line}");
    }
    // With a file, as `:sp {file}`.
    assert_eq!(host("new x.rs"), split(false, Some("x.rs")));
    assert_eq!(host("vnew x.rs"), split(true, Some("x.rs")));
}

#[test]
fn only_and_close() {
    for line in ["on", "onl", "only", "on \"x", "only  "] {
        assert_eq!(host(line), WindowCommand::Only { force: false }, ":{line}");
    }
    assert_eq!(host("on!"), WindowCommand::Only { force: true });
    assert_eq!(host("only!"), WindowCommand::Only { force: true });
    for line in ["clo", "clos", "close", "clo \"x"] {
        assert_eq!(host(line), WindowCommand::Close { force: false }, ":{line}");
    }
    assert_eq!(host("clo!"), WindowCommand::Close { force: true });
    assert_eq!(host("close!"), WindowCommand::Close { force: true });

    let trailing = |x: &str| Err(ExError::Trailing(x.into()));
    assert_eq!(parse("only x"), trailing("x"));
    assert_eq!(parse("close x"), trailing("x"));
    // A window number, before or after, names another window.
    for line in ["2on", "on 2", "2clo", "close 2", "%clo"] {
        assert!(
            matches!(parse(line), Err(ExError::Unsupported(_))),
            ":{line}"
        );
    }
}

#[test]
fn other_names_are_not_window_commands() {
    let names = |line: &str| match parse(line) {
        Ok(ExCommand::Host(HostCommand::Window(command))) => Some(command),
        _ => None,
    };
    // `:sview` and `:sfind` stay E492.
    for line in ["sv", "sview x", "sf x", "sfind x"] {
        assert_eq!(
            parse(line),
            Err(ExError::NotACommand(line.into())),
            ":{line}"
        );
    }
    for line in ["s", "v/x/d", "n", "ne", "vn", "o", "cl", "s/a/b/"] {
        assert_eq!(names(line), None, ":{line}");
    }
}

#[test]
fn unsupported_forms() {
    for line in [
        "sp +1 x",
        "sp ++ff=unix x",
        "sp | e x",
        "on | e x",
        "new | d",
    ] {
        assert!(
            matches!(parse(line), Err(ExError::Unsupported(_))),
            ":{line} gave {:?}",
            parse(line)
        );
    }
    // A command before one runs first.
    let commands = Ex::commands("2 | sp");
    assert_eq!(commands, ["2", " sp"]);
}

#[test]
fn the_line_leaves_the_command_for_the_host() {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(":vsp other.rs<CR>").unwrap();
    assert_eq!(vim.state().message(), None);
    assert_eq!(
        vim.state_mut().take_host_command(),
        Some(HostCommand::Window(split(true, Some("other.rs"))))
    );

    // Run by `:g` or `:normal`, the rest would run in the window left.
    for line in [":g/one/sp<CR>", ":normal :sp<lt>CR><CR>"] {
        let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
        vim.feed(line).unwrap();
        assert_eq!(vim.state_mut().take_host_command(), None, "{line}");
        let message = vim.state().message().map(String::from);
        assert!(
            message
                .as_deref()
                .is_some_and(|m| m.contains("window command")),
            "{line}: {message:?}"
        );
    }
}
