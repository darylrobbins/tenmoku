//! The bundled `lazyvim` preset's `<leader>d` debug keys, and `<leader>td`.

use std::sync::Arc;

use fe_vim::{HeadlessVim, RemapSet};

use super::*;

fn vim() -> HeadlessVim {
    let preset = bundled(VimPreset::Lazyvim).unwrap();
    let mut vim = HeadlessVim::new("one\ntwo", (0, 0));
    let (set, issues) = RemapSet::new(&parse_with_leader(preset.text, "<Space>").remaps, "<Space>");
    assert_eq!(issues, []);
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

/// The id of the next command a map ran, as the host would dispatch it.
fn taken(vim: &mut HeadlessVim) -> Option<String> {
    let command = vim.state_mut().take_remap_command()?;
    Some(command.0.command.to_string())
}

#[test]
fn each_debug_key_dispatches_its_command() {
    let mut vim = vim();
    for (keys, command) in [
        ("<Space>db", "debug.toggle_breakpoint"),
        ("<Space>dc", "debug.start"),
        ("<Space>da", "debug.start_with_args"),
        ("<Space>dC", "debug.run_to_cursor"),
        ("<Space>di", "debug.step_into"),
        ("<Space>do", "debug.step_out"),
        ("<Space>dO", "debug.step_over"),
        ("<Space>dj", "debug.frame_down"),
        ("<Space>dk", "debug.frame_up"),
        ("<Space>dl", "debug.rerun_last"),
        ("<Space>dP", "debug.pause"),
        ("<Space>dr", "debug.show_console"),
        ("<Space>ds", "debug.show_session"),
        ("<Space>dt", "debug.stop"),
        ("<Space>dw", "debug.hover"),
        ("<Space>du", "debug.show_panels"),
        ("<Space>de", "debug.evaluate"),
    ] {
        vim.feed(keys).unwrap();
        assert_eq!(taken(&mut vim).as_deref(), Some(command), "{keys}");
    }
}

#[test]
#[allow(non_snake_case)]
fn leader_do_steps_out_and_leader_dO_steps_over() {
    let mut vim = vim();
    vim.feed("<Space>do").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("debug.step_out"));
    vim.feed("<Space>dO").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("debug.step_over"));
}

#[test]
fn leader_de_is_mapped_in_normal_and_visual() {
    let mut vim = vim();
    vim.feed("<Space>de").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("debug.evaluate"));
    vim.feed("v<Space>de").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("debug.evaluate"));
}

#[test]
fn leader_td_dispatches_debug_test_at_cursor() {
    let mut vim = vim();
    vim.feed("<Space>td").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("debug.test_at_cursor"));
}
