//! The quickfix and location lists' windows: `:copen`, `:cwindow`,
//! `:cclose` and their `:l` twins as the host receives them, a
//! `HostCommand::Quickfix` that shows or hides the list, abbreviated and
//! checked as nvim 0.12.4 does (`:copen 0` is E939, `:copen x` E488,
//! `:copen!` E477, `:2cclose` E481).

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

fn command(action: QuickfixAction, location: bool) -> Option<QuickfixCommand> {
    Some(QuickfixCommand { action, location })
}

#[test]
fn copen_and_lopen_parse_to_quickfix_open() {
    let open = |location| command(QuickfixAction::Open, location);
    for line in [
        "cope",
        "copen",
        "cw",
        "cwin",
        "cwindow",
        "copen 5",
        "copen \"x",
    ] {
        assert_eq!(quickfix(line), open(false), ":{line}");
    }
    for line in ["lop", "lope", "lopen", "lw", "lwindow", "lopen 3"] {
        assert_eq!(quickfix(line), open(true), ":{line}");
    }
    // Any range is a height, as nvim takes it.
    for line in ["5copen", "0copen", ".copen", "%copen", "1,2copen", "3lop"] {
        assert!(quickfix(line).is_some(), ":{line}");
    }
    let some = |text: &str| Some(text.to_owned());
    assert_eq!(error("copen!"), some("E477: No ! allowed"));
    assert_eq!(error("cw!"), some("E477: No ! allowed"));
    assert_eq!(error("copen 0"), some("E939: Positive count required"));
    assert_eq!(error("copen x"), some("E488: Trailing characters: x"));
    assert_eq!(error("copen 3 4"), some("E488: Trailing characters: 4"));
    assert_eq!(error("cwindow x"), some("E488: Trailing characters: x"));
    // `:cop` is still `:copy`, `:co` too.
    assert_eq!(quickfix("cop 0"), None);
}

#[test]
fn cclose_and_lclose_parse_to_quickfix_close() {
    let close = |location| command(QuickfixAction::Close, location);
    for line in ["ccl", "cclo", "cclose", "cclose  ", "cclose \" a comment"] {
        assert_eq!(quickfix(line), close(false), ":{line}");
    }
    for line in ["lcl", "lclo", "lclose"] {
        assert_eq!(quickfix(line), close(true), ":{line}");
    }
    let some = |text: &str| Some(text.to_owned());
    assert_eq!(error("cclose!"), some("E477: No ! allowed"));
    assert_eq!(error("lclose!"), some("E477: No ! allowed"));
    assert_eq!(error("2cclose"), some("E481: No range allowed"));
    assert_eq!(error(".lcl"), some("E481: No range allowed"));
    assert_eq!(error("cclose x"), some("E488: Trailing characters: x"));
}

/// `:vimgrep`'s pattern doesn't end at a `]` or delimiter inside a POSIX
/// class, an equivalence or collating member, or a nested set within a
/// collection (T482 F5): `[[:alpha:]/]` is one collection.
#[test]
fn vimgrep_collections_hold_classes_and_nested_sets() {
    let split = |args: &str| QuickfixAction::parse_vimgrep(args);
    let parts = |pattern: &str, flags: &str, globs: &[&str]| {
        let globs = globs.iter().map(|glob| (*glob).to_owned()).collect();
        Ok((pattern.to_owned(), flags.to_owned(), globs))
    };
    assert_eq!(
        split("/a[[:alpha:]/]b/g x"),
        parts("a[[:alpha:]/]b", "g", &["x"])
    );
    assert_eq!(split("/[^[:space:]]/ x"), parts("[^[:space:]]", "", &["x"]));
    assert_eq!(split("/[[=e=]/]/ x"), parts("[[=e=]/]", "", &["x"]));
    assert_eq!(split("/[[.-.]/]/ x"), parts("[[.-.]/]", "", &["x"]));
    assert_eq!(split("/[a[b/]c]/j x"), parts("[a[b/]c]", "j", &["x"]));
    assert_eq!(split("/[a[^]/]]/ x"), parts("[a[^]/]]", "", &["x"]));
    // A `[` that opens nothing closed is a plain member, as before.
    assert_eq!(split("/[[]/ x"), parts("[[]", "", &["x"]));
    assert_eq!(split("/[[:/]/ x"), parts("[[:/]", "", &["x"]));
    assert_eq!(split("/[[:alpha]/ x"), parts("[[:alpha]", "", &["x"]));
    // Deeply nested, unclosed sets don't recurse without bound.
    let deep = "[".repeat(5_000);
    assert_eq!(split(&format!("/{deep}/ x")), parts(&deep, "", &["x"]));
    let closed = format!("{deep}{}", "]".repeat(5_000));
    assert_eq!(split(&format!("/{closed}/ x")), parts(&closed, "", &["x"]));
}
