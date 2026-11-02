//! `r<CR>` and `gi` against recorded Neovim: `tests/corpus/replace_break_and_gi.toml`
//! and its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes.

#[allow(dead_code)]
mod support;

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::{EditorModel, commands::HeadlessHost};
use fe_vim::{Registers, VimState, VimTarget, context, registry};
use support::{check, load};

#[test]
fn replace_break_and_gi_match_neovim() {
    let recorded = load("replace_break_and_gi").unwrap();
    assert!(
        recorded.len() >= 20,
        "replace_break_and_gi: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "r_cr",
        "r_ctrl_j",
        "r_register",
        "r_blank_after_dropped",
        "r_blank_before_kept",
        "r_indented",
        "r_tab_indented",
        "r_count",
        "r_last_char",
        "r_dot",
        "r_undo",
        "r_count_too_few",
        "r_within_indent",
        "r_end_of_indent",
        "r_whitespace_only_row",
        "r_whitespace_only_row_last",
        "r_suffix_exhausted",
        "r_suffix_exhausted_count",
        "r_tab_within_indent",
        "gi_no_insert_yet",
        "gi_after_A",
        "gi_count",
        "gi_after_O",
        "gi_after_rows_put_above",
        "gi_after_cw",
        "gi_after_R",
        "gi_dot",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "replace_break_and_gi: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "replace_break_and_gi: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// `vim.replace_char` with a carriage return does nothing (fe-text is
/// LF-only), through the public command API.
#[test]
fn replace_char_ignores_a_carriage_return() {
    let mut model = EditorModel::new(Buffer::new("foo bar"), None);
    model.set_cursor(3);
    let mut state = VimState::new();
    let mut host = HeadlessHost::new();
    let mut registers = Registers::new();
    let registry = registry();
    let invocation = Invocation::of("vim.replace_char")
        .unwrap()
        .args(serde_json::json!({"char": "\r"}))
        .context(context(&state));
    let mut target = VimTarget {
        model: &mut model,
        host: &mut host,
        state: &mut state,
        registers: &mut registers,
    };
    let _ = registry.dispatch(&mut target, &invocation);
    assert_eq!(model.text().to_string(), "foo bar");
    assert!(!model.text().to_string().contains('\r'));
}
