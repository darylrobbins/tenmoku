//! `:r[ead] [file]` as the host receives it.

use fe_vim::{HeadlessVim, HostCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("a\nb\nc", (1, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn read(path: Option<&str>, below: u32) -> Option<HostCommand> {
    Some(HostCommand::Read {
        path: path.map(String::from),
        below,
    })
}

#[test]
fn read_parses_to_a_host_command() {
    assert_eq!(run("r f.txt").0, read(Some("f.txt"), 2));
    assert_eq!(run("read f.txt").0, read(Some("f.txt"), 2));
    assert_eq!(run("3r f.txt").0, read(Some("f.txt"), 3));
    assert_eq!(run("0r f.txt").0, read(Some("f.txt"), 0));
    assert_eq!(run("r").0, read(None, 2));
    // The shell's output is the host's too (`tests/filter.rs`).
    assert!(matches!(
        run("r !ls").0,
        Some(HostCommand::ReadCommand { below: 2, .. })
    ));
    assert_eq!(run("rege").0, None);
    assert!(run("rege").1.unwrap().starts_with("E492"));
}
