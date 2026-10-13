//! fe-vim's `@:` and `Q` against recorded Neovim:
//! `tests/corpus/repeat_command_line.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use fe_vim::{CommandLineKind, HeadlessVim};
use support::{check, load};

#[test]
fn repeat_command_line_matches_neovim() {
    let recorded = load("repeat_command_line").unwrap();
    assert!(
        recorded.len() >= 15,
        "repeat_command_line: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "at_colon_after_s",
        "at_colon_delete_range",
        "at_colon_failing_line",
        "count_at_colon",
        "at_at_after_at_colon",
        "at_colon_after_esc_cancel",
        "at_colon_in_macro",
        "Q_after_record",
        "qA_append_then_Q",
        "Q_count",
        "Q_then_at_at",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "repeat_command_line: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "repeat_command_line: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn no_previous_line() {
    let mut vim = HeadlessVim::new("abc\ndef", (0, 0));
    vim.feed("@:").unwrap();
    assert_eq!(vim.state().message(), Some("E30: No previous command line"));
    assert_eq!(vim.text(), "abc\ndef");
    assert_eq!(vim.registers().history(CommandLineKind::Ex).count(), 0);
    // A line that ran is repeated without a new history entry.
    vim.feed(":s/b/X/<CR>@:").unwrap();
    assert_eq!(vim.registers().history(CommandLineKind::Ex).count(), 1);
}
