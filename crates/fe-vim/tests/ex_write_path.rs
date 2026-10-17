//! `:w {path}` and `:sav[eas] {path}` as the host receives them.

use fe_vim::{HeadlessVim, HostCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn write(force: bool, path: &str) -> Option<HostCommand> {
    Some(HostCommand::Write {
        force,
        path: Some(path.into()),
    })
}

fn save_as(force: bool, path: &str) -> Option<HostCommand> {
    Some(HostCommand::SaveAs {
        force,
        path: path.into(),
    })
}

#[test]
fn parses() {
    assert_eq!(run("w a.txt").0, write(false, "a.txt"));
    assert_eq!(run(r"w! a\ b.txt").0, write(true, "a b.txt"));
    assert_eq!(run("sav x").0, save_as(false, "x"));
    assert_eq!(run("saveas! x").0, save_as(true, "x"));
    assert_eq!(run("savea  y ").0, save_as(false, "y"));
    assert_eq!(
        run("sav"),
        (None, Some("E471: Argument required".to_string()))
    );
    assert_eq!(run("wq a").0, None);
    assert_eq!(run("x a").0, None);
}
