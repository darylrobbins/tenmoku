//! The day-one features' ex commands as the host receives them: netrw's
//! `:Ex`, `:oldfiles`, the quickfix and location list's `:copen` and
//! `:cclose`, and `:Replace` across files.

use fe_vim::{HeadlessVim, HostCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn host(line: &str) -> Option<HostCommand> {
    let (command, message) = run(line);
    assert_eq!(message, None, ":{line}");
    command
}

fn error(line: &str) -> String {
    let (command, message) = run(line);
    assert_eq!(command, None, ":{line}");
    message.unwrap_or_else(|| panic!(":{line} gave no message"))
}

fn replace(text: &str) -> Option<HostCommand> {
    Some(HostCommand::ReplaceInFiles { text: text.into() })
}

#[test]
fn explore() {
    for line in ["Ex", "Exp", "Explore", "Explore  "] {
        assert_eq!(host(line), Some(HostCommand::Explore), ":{line}");
    }
    assert_eq!(error("Ex foo"), "E488: Trailing characters: foo");
    assert_eq!(error("1Ex"), "E481: No range allowed");
    assert_eq!(error("%Explore"), "E481: No range allowed");
    assert_eq!(error("Ex!"), "E477: No ! allowed");
    // `:ex` is still `:edit`.
    assert_eq!(
        host("ex"),
        Some(HostCommand::Edit {
            path: None,
            force: false
        })
    );
}

#[test]
fn oldfiles() {
    for line in [
        "ol",
        "old",
        "oldfiles",
        "bro ol",
        "browse oldfiles",
        "brow  old",
    ] {
        assert_eq!(host(line), Some(HostCommand::OldFiles), ":{line}");
    }
    assert_eq!(error("2ol"), "E481: No range allowed");
    assert_eq!(error("2bro ol"), "E481: No range allowed");
    assert_eq!(error("ol x"), "E488: Trailing characters: x");
    assert_eq!(error("bro"), "E471: Argument required");
    assert!(error("o").starts_with("E492"));
}

#[test]
fn problems() {
    for line in ["cope", "copen", "lop", "lope", "lopen"] {
        assert_eq!(host(line), Some(HostCommand::OpenProblems), ":{line}");
    }
    for line in ["ccl", "cclo", "cclose", "lcl", "lclose"] {
        assert_eq!(host(line), Some(HostCommand::CloseProblems), ":{line}");
    }
    assert_eq!(error("1,2copen"), "E481: No range allowed");
    assert_eq!(error(".ccl"), "E481: No range allowed");
    assert_eq!(error("copen!"), "E477: No ! allowed");
    assert_eq!(error("cclose x"), "E488: Trailing characters: x");
    // `:cop` is still `:copy`, which a host doesn't see.
    assert_eq!(run("cop 0").0, None);
}

#[test]
fn replace_in_files() {
    assert_eq!(host("Replace x"), replace("x"));
    assert_eq!(host("Replace   bar baz  "), replace("bar baz"));
    assert_eq!(host("Replace\tx\"y"), replace("x\"y"));
    assert_eq!(error("Replace"), "E471: Argument required");
    assert_eq!(error("Replace   "), "E471: Argument required");
    assert_eq!(error("%Replace x"), "E481: No range allowed");
    assert_eq!(error("Replace! x"), "E477: No ! allowed");
    // Another user command, named longer, is still the host's to know.
    assert_eq!(
        host("Replaced x"),
        Some(HostCommand::User {
            name: "Replaced".into(),
            bang: false,
            range: None,
            args: "x".into()
        })
    );
}
