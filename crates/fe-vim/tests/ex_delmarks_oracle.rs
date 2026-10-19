//! fe-vim's `:delmarks` and `:clearjumps` against recorded Neovim:
//! `tests/corpus/ex_delmarks.toml` and its expectations, which the ignored
//! recorder in `tests/oracle.rs` writes; and the messages of the errors.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn ex_delmarks_matches_neovim() {
    let recorded = load("ex_delmarks").unwrap();
    assert!(
        recorded.len() >= 10,
        "ex_delmarks: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "delmarks_one",
        "delmarks_range",
        "delmarks_spaced",
        "delmarks_bang",
        "delmarks_jump_mark",
        "delmarks_visual_marks",
        "clearjumps_then_ctrl_o_fails",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "ex_delmarks: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex_delmarks: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// Feeds `:{line}` to a buffer with marks `a` and `b` set, and returns the
/// message left, whether the key failed, and whether `'a` and `'b` still
/// go anywhere.
fn run(line: &str) -> (Option<String>, bool, [bool; 2]) {
    let mut vim = HeadlessVim::new("one\ntwo\nthree", (0, 0));
    vim.feed("majmbgg").unwrap();
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    let failed = vim.state().failed();
    let set = ['a', 'b'].map(|name| vim.mark(name).is_some());
    (message, failed, set)
}

#[test]
fn delmarks_messages() {
    // nvim 0.12.4 `--clean --headless`, `pcall(vim.cmd, ...)` on the same
    // buffer: the same errors, and a mark deleted before a bad one is gone.
    // `:delmarks 1` is no error in nvim (it has the marks `0` to `9`).
    for (line, message, set) in [
        ("delmarks", "E471: Argument required", [true, true]),
        ("delmarks!  a", "E474: Invalid argument", [true, true]),
        ("delmarks a-", "E475: Invalid argument: a-", [true, true]),
        ("delmarks c-a", "E475: Invalid argument: c-a", [true, true]),
        ("delmarks a-B", "E475: Invalid argument: a-B", [true, true]),
        ("delmarks a,b", "E475: Invalid argument: ,b", [false, true]),
        (
            "delmarks b-c-e",
            "E475: Invalid argument: -e",
            [true, false],
        ),
        ("delmarks '", "E475: Invalid argument: '", [true, true]),
        ("2delmarks a", "E481: No range allowed", [true, true]),
    ] {
        let (got, failed, now) = run(line);
        assert_eq!(got.as_deref(), Some(message), ":{line}");
        assert!(failed, ":{line}");
        assert_eq!(now, set, ":{line}");
    }
    for line in ["delmarks 1", "delmarks z", "clearjumps"] {
        let (message, failed, set) = run(line);
        assert_eq!(
            (message, failed, set),
            (None, false, [true, true]),
            ":{line}"
        );
    }
}
