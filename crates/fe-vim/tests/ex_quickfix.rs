//! The quickfix and location list commands as the host receives them: a
//! `HostCommand::Quickfix`, abbreviated as nvim 0.12.4 does
//! (`fullcommand()`: `:cne` is `:cnext`, `:cnew` `:cnewer`, `:cl`
//! `:clist`, `:ln` is `:lnoremap`, `:cf` `:cfile`).

use fe_vim::{HeadlessVim, HostCommand, QuickfixAction, QuickfixCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn quickfix(line: &str) -> Option<QuickfixCommand> {
    match run(line) {
        (Some(HostCommand::Quickfix(command)), None) => Some(command),
        _ => None,
    }
}

/// The message `line` fails with, if it hands the host nothing.
fn error(line: &str) -> Option<String> {
    match run(line) {
        (None, message) => message,
        _ => None,
    }
}

fn c(action: QuickfixAction) -> Option<QuickfixCommand> {
    Some(QuickfixCommand {
        action,
        location: false,
    })
}

fn l(action: QuickfixAction) -> Option<QuickfixCommand> {
    Some(QuickfixCommand {
        action,
        location: true,
    })
}

#[test]
fn cnext_and_friends_parse_to_quickfix_commands() {
    use QuickfixAction::*;
    let cases: &[(&[&str], &[&str], QuickfixAction)] = &[
        (
            &["cn", "cne", "cnext", "cn!"],
            &["lne", "lnext", "lne!"],
            Next(1),
        ),
        (
            &["cnext 3", "3cnext", "3cn", "cn! 3"],
            &["lnext 3", "3lne"],
            Next(3),
        ),
        (
            &["cp", "cprevious", "cN", "cNext", "cpr"],
            &["lp", "lprevious", "lN", "lNext"],
            Previous(1),
        ),
        (&["cp 2", "2cN"], &["lp 2", "2lN"], Previous(2)),
        (
            &["cfir", "cfirst", "cr", "crewind"],
            &["lfir", "lfirst", "lr"],
            First,
        ),
        (&["cla", "clast", "cla!"], &["lla", "llast"], Last),
        (&["cc", "cc!"], &["ll", "ll!"], Go(None)),
        (
            &["cc 4", "4cc", "cfir 4", "4cla"],
            &["ll 4", "4ll", "lla 4"],
            Go(Some(4)),
        ),
        (&["cnf", "cnfile"], &["lnf", "lnfile"], NextFile(1)),
        (&["cnf 2", "2cnf"], &["lnf 2"], NextFile(2)),
        (
            &["cNf", "cNfile", "cpf", "cpfile"],
            &["lNf", "lpf", "lpfile"],
            PreviousFile(1),
        ),
        (&["col", "colder"], &["lol", "lolder"], Older(1)),
        (
            &["col 2", "2col", "colder 2"],
            &["lol 2", "2lolder"],
            Older(2),
        ),
        (&["cnew", "cnewer"], &["lnew", "lnewer"], Newer(1)),
        (&["cnew 3", "3cnewer"], &["lnew 3"], Newer(3)),
        (&["cl", "cli", "clist", "cl!"], &["lli", "llist"], List),
    ];
    for (quickfix_forms, location_forms, action) in cases {
        for line in *quickfix_forms {
            assert_eq!(quickfix(line), c(action.clone()), ":{line}");
        }
        for line in *location_forms {
            assert_eq!(quickfix(line), l(action.clone()), ":{line}");
        }
    }
    // A comment ends the line; blanks around the count are fine.
    assert_eq!(quickfix("cnext  2 \" two"), c(Next(2)));
    // A range's number and a count after it add, less one, as nvim's do.
    assert_eq!(quickfix("2cnext 3"), c(Next(4)));
    // A range's number is a count, not a line: past the last line is fine.
    assert_eq!(quickfix("500cnext"), c(Next(500)));
    // A range of 0 moves one item, as nvim's does.
    assert_eq!(quickfix("0cnext"), c(Next(1)));
    assert_eq!(
        quickfix("cnext 99999999999"),
        c(Next(i32::MAX as u32)),
        "the largest past 32 bits"
    );
    // Not ours: `:cf` is `:cfile`, `:co` `:copy`, `:ln` `:lnoremap`, `:l`
    // `:list`, and `:clo` closes the window.
    for line in ["cf", "co", "ln", "l", "lf", "grepa x"] {
        assert_eq!(quickfix(line), None, ":{line}");
    }
    assert!(matches!(run("clo").0, Some(HostCommand::Window(_))));
    // `:copen` and friends show and hide the list (`ex_quickfix_window.rs`).
    assert_eq!(quickfix("copen"), c(Open));
    assert_eq!(quickfix("lop"), l(Open));
    assert_eq!(quickfix("ccl"), c(Close));
    assert_eq!(quickfix("lcl"), l(Close));
}

#[test]
fn fill_commands_carry_their_argument_text() {
    use QuickfixAction::*;
    assert_eq!(quickfix("grep foo src/"), c(Grep("foo src/".into())));
    assert_eq!(quickfix("gr! -w \"x\" | y"), c(Grep("-w \"x\" | y".into())));
    assert_eq!(quickfix("lgr foo"), l(Grep("foo".into())));
    assert_eq!(quickfix("vim /a|b/ **"), c(VimGrep("/a|b/ **".into())));
    assert_eq!(quickfix("lv /a/ %"), l(VimGrep("/a/ %".into())));
    assert_eq!(quickfix("mak"), c(Make(String::new())));
    assert_eq!(quickfix("make -j4 "), c(Make("-j4".into())));
    assert_eq!(quickfix("lmak test"), l(Make("test".into())));
    for line in ["grep", "lgrep ", "vimgrep", "lvimgrep"] {
        assert_eq!(
            error(line).as_deref(),
            Some("E471: Argument required"),
            ":{line}"
        );
    }
}

#[test]
fn quickfix_commands_refuse_other_ranges() {
    let no_range = Some("E481: No range allowed");
    for line in [
        "%cnext", ".cnext", "$cn", "1,2cnext", "+cn", "2;3cp", "1+1cn", "%cc", "1,2col", "2cl",
        "3llist", "%lne", "2grep x", "1make",
    ] {
        assert_eq!(error(line).as_deref(), no_range, ":{line}");
    }
    // A mark is read (E20 if unset) before the command refuses it.
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed("ma:'acn<CR>").unwrap();
    assert_eq!(vim.state_mut().take_host_command(), None);
    assert_eq!(vim.state().message(), no_range);
    assert_eq!(
        error("cnext 1 2").as_deref(),
        Some("E488: Trailing characters: 2")
    );
    assert_eq!(
        error("cn x").as_deref(),
        Some("E488: Trailing characters: x")
    );
    for line in ["cnext 0", "cc 0", "col 0", "lnew 0"] {
        assert_eq!(
            error(line).as_deref(),
            Some("E939: Positive count required"),
            ":{line}"
        );
    }
    assert_eq!(error("0cc").as_deref(), Some("E16: Invalid range"));
    // The host lands after the line runs, so nothing may follow.
    assert!(error("cnext | d").is_some_and(|message| message.contains("not supported")));
    assert!(error("clist 1,3").is_some_and(|message| message.contains("not supported")));
}

#[test]
fn colder_and_cnewer_take_counts() {
    use QuickfixAction::*;
    assert_eq!(quickfix("colder 3"), c(Older(3)));
    assert_eq!(quickfix("4col"), c(Older(4)));
    assert_eq!(quickfix("cnewer 2"), c(Newer(2)));
    assert_eq!(quickfix("lolder 5"), l(Older(5)));
    assert_eq!(quickfix("2lnew"), l(Newer(2)));
}

#[test]
fn quickfix_commands_are_refused_under_g_and_normal() {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(":g/one/cnext<CR>").unwrap();
    assert_eq!(vim.state_mut().take_host_command(), None);
    let message = vim.state().message().unwrap_or_default().to_owned();
    assert!(message.contains("a quickfix command in :g"), "{message}");
}
