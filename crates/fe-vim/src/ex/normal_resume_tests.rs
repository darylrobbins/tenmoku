//! A `:normal` that reaches a map's `<Cmd>fe:…<CR>` suspends there when the
//! host resumes it, and goes on, with the keys and range rows left, once
//! the host has run the command.

use std::sync::Arc;

use fe_command::{Invocation, Source};

use crate::{HeadlessVim, Remap, RemapCommand, RemapMode, RemapSet, context};

fn vim_text(text: &str) -> HeadlessVim {
    vim(text, true)
}

fn vim(text: &str, resumes: bool) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    let map = |from: &str, to: &str| Remap {
        modes: vec![RemapMode::Normal],
        from: from.into(),
        to: to.into(),
        recursive: false,
    };
    let maps = [
        map("Z", "<Cmd>fe:editor.save<CR>"),
        map(
            "T",
            "<Cmd>fe:editor.save<CR><Cmd>fe:editor.save<CR>ibar<Esc>",
        ),
        map("W", "<Cmd>fe:editor.save<CR>x"),
        map("x", "iQ<Esc>"),
    ];
    let (set, issues) = RemapSet::new(&maps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(Arc::new(set));
    vim.state_mut().set_normal_resumes(resumes);
    vim
}

fn save() -> RemapCommand {
    RemapCommand(
        Invocation::of("editor.save")
            .unwrap()
            .source(Source::Keyboard),
    )
}

/// Runs `text` as an ex line typed on the command line, whose `<Esc>`s are
/// the `:normal`'s keys, not typed ones.
fn submit(vim: &mut HeadlessVim, text: &str) {
    vim.feed(":").unwrap();
    let submit = Invocation::of("vim.command_line_submit")
        .expect("a valid command ID")
        .args(serde_json::json!({ "text": text }))
        .context(context(vim.state()));
    vim.dispatch(&submit).expect("submits");
}

/// What the host does once the command it took has run.
fn resume(vim: &mut HeadlessVim) {
    let resume = Invocation::of("vim.normal_resume")
        .expect("a valid command ID")
        .context(context(vim.state()));
    vim.dispatch(&resume).expect("resumes");
}

#[test]
fn an_fe_piece_in_the_middle_of_normal_suspends_and_resumes() {
    let mut vim = vim("one\ntwo", true);
    submit(&mut vim, "normal ifoo<Esc>Zibar<Esc>");

    // The keys before it ran; those after it wait for the command.
    assert_eq!(vim.text(), "fooone\ntwo");
    assert!(vim.state().has_normal_suspended());
    assert!(!vim.state().failed());
    assert_eq!(vim.state().message(), None);
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));
    assert_eq!(vim.state_mut().take_remap_command(), None);

    resume(&mut vim);
    assert_eq!(vim.text(), "fobaroone\ntwo");
    assert_eq!(vim.mode(), "normal");
    assert!(!vim.state().has_normal_suspended());
    assert_eq!(vim.state_mut().take_remap_command(), None);

    // With nothing suspended, resuming does nothing.
    resume(&mut vim);
    assert_eq!(vim.text(), "fobaroone\ntwo");
}

#[test]
fn a_ranged_normal_resumes_each_row_in_order() {
    let mut vim = vim("a\nb\nc", true);
    submit(&mut vim, "%normal Ax<Esc>ZAy<Esc>");
    assert_eq!(vim.text(), "ax\nb\nc");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));

    resume(&mut vim);
    assert_eq!(vim.text(), "axy\nbx\nc");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));

    resume(&mut vim);
    assert_eq!(vim.text(), "axy\nbxy\ncx");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));

    resume(&mut vim);
    assert_eq!(vim.text(), "axy\nbxy\ncxy");
    assert_eq!(vim.state_mut().take_remap_command(), None);
    assert!(!vim.state().has_normal_suspended());
}

#[test]
fn a_host_that_does_not_resume_gets_the_refusal() {
    let mut vim = vim("one", false);
    submit(&mut vim, "normal ifoo<Esc>Zibar<Esc>");
    assert_eq!(vim.text(), "fooone");
    assert!(!vim.state().has_normal_suspended());
    assert_eq!(vim.state_mut().take_remap_command(), None);
    assert_eq!(
        vim.state().message(),
        Some("fe: commands don't run inside :normal")
    );
}

#[test]
fn a_dropped_replay_drops_the_suspended_normal() {
    let mut vim = vim("one", true);
    submit(&mut vim, "normal ifoo<Esc>Zibar<Esc>");
    vim.state_mut().stop_replay();
    assert!(!vim.state().has_normal_suspended());
    resume(&mut vim);
    assert_eq!(vim.text(), "fooone");
}

#[test]
fn a_mapped_expansion_resumes_with_its_tags() {
    // Two commands in one map: each suspends in turn, then the keys after.
    let mut vim = vim("one", true);
    submit(&mut vim, "normal T");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));
    resume(&mut vim);
    assert_eq!(vim.text(), "one");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));
    resume(&mut vim);
    assert_eq!(vim.text(), "barone");
    assert_eq!(vim.state_mut().take_remap_command(), None);

    // The map's keys after the command are its nonrecursive expansion: the
    // `x` isn't mapped, as a typed one would be.
    let mut vim = vim_text("abc");
    submit(&mut vim, "normal W");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));
    resume(&mut vim);
    assert_eq!(vim.text(), "bc");
}
