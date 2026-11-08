//! `:grep` with no pattern is the last search's, which the host resolves
//! (E35 with none); `:vimgrep` with none stays E471.

use fe_vim::{HeadlessVim, HostCommand, QuickfixAction};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

#[test]
fn an_empty_grep_reuses_the_last_search() {
    for (line, location) in [("grep", false), ("gr ", false), ("lgrep", true)] {
        let (command, message) = run(line);
        let Some(HostCommand::Quickfix(command)) = command else {
            panic!(":{line} handed the host nothing: {message:?}");
        };
        assert_eq!(
            command.action,
            QuickfixAction::Grep(String::new()),
            ":{line}"
        );
        assert_eq!(command.location, location, ":{line}");
    }
    for line in ["vimgrep", "lvimgrep "] {
        let (command, message) = run(line);
        assert!(command.is_none(), ":{line}");
        assert_eq!(
            message.as_deref(),
            Some("E471: Argument required"),
            ":{line}"
        );
    }
}
