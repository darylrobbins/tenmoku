//! The tab commands as the host receives them: a `HostCommand::Tab`,
//! abbreviated as nvim 0.12.4 does (`fullcommand()`: `tabnew` has no
//! shorter form, as `:tabne` is `:tabnext`; `:tabf` is `:tabfind`).

use fe_vim::{HeadlessVim, HostCommand, TabCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn tab(line: &str) -> Option<TabCommand> {
    match run(line) {
        (Some(HostCommand::Tab(command)), None) => Some(command),
        _ => None,
    }
}

fn new(file: Option<&str>) -> Option<TabCommand> {
    Some(TabCommand::New {
        file: file.map(String::from),
    })
}

#[test]
fn tabnew_and_tabedit_open_a_tab() {
    assert_eq!(tab("tabnew"), new(None));
    assert_eq!(tab("tabnew a.txt"), new(Some("a.txt")));
    assert_eq!(tab("tabnew!  a\\ b.txt "), new(Some("a b.txt")));
    for line in ["tabe", "tabed", "tabedit"] {
        assert_eq!(tab(line), new(None), ":{line}");
    }
    assert_eq!(tab("tabe x.rs"), new(Some("x.rs")));
    // `:tabne` is `:tabnext`.
    assert_eq!(tab("tabne"), Some(TabCommand::Next(None)));
}

#[test]
fn next_and_previous_take_counts() {
    for line in ["tabn", "tabnext", "tabnex"] {
        assert_eq!(tab(line), Some(TabCommand::Next(None)), ":{line}");
    }
    assert_eq!(tab("tabn 3"), Some(TabCommand::Next(Some(3))));
    assert_eq!(tab("2tabn"), Some(TabCommand::Next(Some(2))));
    for line in ["tabp", "tabpr", "tabprevious", "tabN", "tabNe", "tabNext"] {
        assert_eq!(tab(line), Some(TabCommand::Previous(1)), ":{line}");
    }
    assert_eq!(tab("tabp 2"), Some(TabCommand::Previous(2)));
    assert_eq!(tab("3tabN"), Some(TabCommand::Previous(3)));
    assert_eq!(
        tab("tabn 99999999999"),
        Some(TabCommand::Next(Some(i32::MAX as u32)))
    );
}

#[test]
fn zero_and_junk_counts_are_errors() {
    for line in ["tabn 0", "0tabn", "tabp 0", "tabn x", "tabn 1 2"] {
        let (command, message) = run(line);
        assert_eq!(command, None, ":{line}");
        assert!(message.is_some(), ":{line}");
    }
}

#[test]
fn close_only_first_last() {
    assert_eq!(tab("tabc"), Some(TabCommand::Close { force: false }));
    assert_eq!(tab("tabclose!"), Some(TabCommand::Close { force: true }));
    assert_eq!(tab("tabo"), Some(TabCommand::Only { force: false }));
    assert_eq!(tab("tabonly!"), Some(TabCommand::Only { force: true }));
    for line in ["tabfir", "tabfirst", "tabr", "tabrewind"] {
        assert_eq!(tab(line), Some(TabCommand::First), ":{line}");
    }
    for line in ["tabl", "tabla", "tablast"] {
        assert_eq!(tab(line), Some(TabCommand::Last), ":{line}");
    }
}

#[test]
fn other_tab_commands_stay_unsupported() {
    for line in [
        "tabm",
        "tabmove",
        "tabdo",
        "tabdo d",
        "tabs",
        "tabf",
        "tabfi",
        "tabc 2",
        "2tabc",
        "2tabo",
        "tabfirst x",
        "tabnew | tabnew",
    ] {
        let (command, message) = run(line);
        assert_eq!(command, None, ":{line}");
        assert!(message.is_some(), ":{line}");
    }
}
