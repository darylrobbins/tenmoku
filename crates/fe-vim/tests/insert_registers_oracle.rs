//! Insert mode's `<C-r>{reg}` (and `<C-r><C-r>`, `<C-r><C-o>`, `<C-r><C-p>`)
//! and `<C-a>` against recorded Neovim: `tests/corpus/insert_registers.toml`
//! and its expectations, which the ignored recorder in `tests/oracle.rs`
//! writes. `<C-r>=` and a key that names no register go through
//! [`HeadlessVim`] alone: they type nothing.

#[allow(dead_code)]
mod support;

use fe_command::Invocation;
use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn insert_registers_match_neovim() {
    let recorded = load("insert_registers").unwrap();
    assert!(
        recorded.len() >= 16,
        "insert_registers: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "ctrl_r_named",
        "ctrl_r_unnamed",
        "ctrl_r_linewise",
        "ctrl_r_multirow_autoindent",
        "ctrl_r_ctrl_r_literal",
        "ctrl_r_ctrl_o_linewise_above",
        "ctrl_r_ctrl_p_fixes_indent",
        "ctrl_r_dot",
        "ctrl_r_colon",
        "ctrl_r_slash",
        "ctrl_r_empty_register",
        "ctrl_r_escape_cancels",
        "ctrl_a_last_insert",
        "ctrl_a_nothing_inserted",
        "ctrl_r_dot_repeat",
        "dot_repeats_ctrl_r_ctrl_o_reads_register_again",
        "dot_repeats_ctrl_r_ctrl_p_reads_register_again",
        "linewise_ctrl_r_ctrl_o_dot_repeat",
        "counted_ctrl_r_insertion",
        "replace_ctrl_r_overwrites",
        "replace_ctrl_r_past_end_of_line",
        "replace_ctrl_r_backspace_restores",
        "replace_ctrl_r_linewise",
        "replace_ctrl_r_ctrl_r_literal",
        "replace_ctrl_r_counted",
        "replace_ctrl_r_dot_repeat",
        "replace_ctrl_r_ctrl_o_linewise_above",
        "linewise_ctrl_r_ctrl_o_consecutive_dots",
        "linewise_ctrl_r_ctrl_p_consecutive_dots",
        "linewise_ctrl_r_ctrl_o_register_changes_between_dots",
        "linewise_ctrl_r_ctrl_p_register_changes_between_dots",
        "linewise_ctrl_r_ctrl_o_counted_dot",
        "linewise_ctrl_r_ctrl_p_counted_dot",
        "replace_ctrl_r_ctrl_o_characterwise_inserts",
        "replace_ctrl_r_ctrl_p_characterwise_inserts",
        "replace_ctrl_r_ctrl_o_characterwise_backspace",
        "replace_ctrl_r_ctrl_p_characterwise_backspace",
        "replace_ctrl_r_ctrl_p_characterwise_indent",
        "replace_ctrl_r_ctrl_o_dot_repeat",
        "replace_ctrl_r_ctrl_p_dot_repeat",
        "replace_ctrl_r_ctrl_o_counted",
        "replace_ctrl_r_ctrl_p_counted",
        "replace_ctrl_r_ctrl_o_consecutive_dots",
        "replace_ctrl_r_ctrl_p_consecutive_dots",
        "replace_ctrl_r_ctrl_o_counted_dot",
        "replace_ctrl_r_ctrl_p_counted_dot",
        "replace_ctrl_r_ctrl_o_linewise_backspace",
        "replace_ctrl_r_ctrl_o_linewise_backspace_twice",
        "replace_ctrl_r_ctrl_p_linewise_backspace",
        "replace_ctrl_r_ctrl_p_linewise_backspace_twice",
        "replace_ctrl_r_multiline_characterwise",
        "replace_ctrl_r_ctrl_r_multiline_characterwise",
        "replace_ctrl_r_multiline_characterwise_backspace",
        "replace_ctrl_r_multiline_characterwise_backspace_across",
        "replace_ctrl_r_ctrl_r_multiline_characterwise_backspace_across",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "insert_registers: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "insert_registers: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn insert_register_refusals_type_nothing() {
    // `=` opens an expression nvim evaluates, which fe-vim has none of; the
    // other keys name no register.
    for keys in [
        "A<C-r>=",
        "A<C-r>!",
        "A<C-r><C-r>=",
        "A<C-r><C-o>!",
        "A<C-r><BS>",
    ] {
        let mut vim = HeadlessVim::new("abc", (0, 0));
        vim.feed("yiw").unwrap();
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), "abc", "{keys}");
        assert_eq!(vim.mode(), "insert", "{keys}");
        assert_eq!(vim.cursor(), (0, 3), "{keys}");
        // Insert mode takes keys as before.
        vim.feed("x<Esc>").unwrap();
        assert_eq!(vim.text(), "abcx", "{keys}");
    }
}

#[test]
fn stale_register_prefix_does_not_outlive_its_session() {
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("yiwA<C-r>").unwrap();
    assert!(vim.state().insert_register_waiting());
    // The app's `<Esc>` binding ends the session without the prefix
    // reading it.
    let invocation = Invocation::of("vim.normal_mode")
        .unwrap()
        .context(fe_vim::context(vim.state()));
    vim.dispatch(&invocation).unwrap();
    assert_eq!(vim.mode(), "normal");
    assert!(!vim.state().insert_register_waiting());
    vim.feed("Ax").unwrap();
    assert!(!vim.state().insert_register_waiting());
    assert_eq!(vim.text(), "abcx");
    assert_eq!(vim.mode(), "insert");
}
