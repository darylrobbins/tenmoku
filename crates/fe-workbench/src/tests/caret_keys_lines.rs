//! Vim's command line and the git commit box's take the caret keys (Left,
//! Right, Home, End, the word motions, Delete) as the other inputs do
//! (T1241), keeping Enter, Esc and Backspace on an empty line: real keys
//! through the default keymap.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{
    EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled, set_vim_remaps,
};
use fe_ui::TextInput;
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{GitPanel, Launch, Workbench};

/// The word-left key: alt on macOS, ctrl elsewhere.
const WORD_LEFT: &str = if cfg!(target_os = "macos") {
    "alt-left"
} else {
    "ctrl-left"
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

fn text(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    input.read_with(cx, |input, _| input.text().to_owned())
}

/// Into the open, empty line `input`: abc, Left, X gives abXc, and so on
/// through Home, End, Right, word-left and Delete; the line stays open.
fn caret_keys(input: &Entity<TextInput>, cx: &mut VisualTestContext) {
    assert_eq!(text(input, cx), "");
    type_text("abc", cx);
    press("left", cx);
    type_text("X", cx);
    assert_eq!(text(input, cx), "abXc");
    press("home", cx);
    type_text("Y", cx);
    assert_eq!(text(input, cx), "YabXc");
    press("end", cx);
    type_text("Z", cx);
    assert_eq!(text(input, cx), "YabXcZ");
    press("home right", cx);
    type_text("R", cx);
    assert_eq!(text(input, cx), "YRabXcZ");
    press("end", cx);
    type_text(" de", cx);
    press(WORD_LEFT, cx);
    type_text("W", cx);
    assert_eq!(text(input, cx), "YRabXcZ Wde");
    press("delete", cx);
    assert_eq!(text(input, cx), "YRabXcZ We");
    press("backspace", cx);
    assert_eq!(text(input, cx), "YRabXcZ e");
}

fn open_with_vim<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    remaps: Vec<Remap>,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    draw(cx);
    (workbench, cx)
}

/// A workbench with `a.txt` open in vim.
fn vim_workbench(
    cx: &mut TestAppContext,
    remaps: Vec<Remap>,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "aaa").unwrap();
    let (workbench, cx) = open_with_vim(cx, dir.path(), remaps);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    (dir, workbench, cx)
}

fn line_input(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TextInput> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.line_input().cloned())
        .expect("the line is open")
}

fn line_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, cx| bar.line(cx).is_some())
}

#[gpui::test]
fn vim_line_takes_caret_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = vim_workbench(cx, Vec::new());
    press(":", cx);
    caret_keys(&line_input(&workbench, cx), cx);
    assert!(line_open(&workbench, cx), "the line stays open");
    // Escape ends it.
    press("escape", cx);
    assert!(!line_open(&workbench, cx));
    // So does Enter.
    press(":", cx);
    type_text("abc", cx);
    press("left", cx);
    assert!(line_open(&workbench, cx));
    press("enter", cx);
    assert!(!line_open(&workbench, cx));
    // And Backspace on an empty line.
    press(":", cx);
    press("backspace", cx);
    assert!(!line_open(&workbench, cx));
}

#[gpui::test]
fn vim_line_resolves_a_held_map_before_a_caret_key(cx: &mut TestAppContext) {
    let map = Remap {
        modes: vec![RemapMode::CmdLine],
        from: "jk".to_string(),
        to: "<CR>".to_string(),
        recursive: false,
    };
    let (_dir, workbench, cx) = vim_workbench(cx, vec![map]);
    press(":", cx);
    type_text("ab", cx);
    cx.simulate_keystrokes("j");
    assert_eq!(text(&line_input(&workbench, cx), cx), "ab", "j is held");
    cx.simulate_keystrokes("left");
    draw(cx);
    // The held j went in first, then the caret moved over it.
    type_text("X", cx);
    assert_eq!(text(&line_input(&workbench, cx), cx), "abXj");
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?} failed");
}

#[gpui::test]
fn git_commit_box_takes_caret_keys(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), "head\n").unwrap();
    let (workbench, cx) = open_with_vim(cx, root, Vec::new());
    dispatch("git.show_changes", cx);
    let panel: Entity<GitPanel> = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    let commit_box: Entity<EditorView> = panel
        .read_with(cx, |panel, _| panel.commit_box().cloned())
        .expect("the panel's drawn with its box");
    cx.update(|window, cx| window.focus(&commit_box.focus_handle(cx), cx));
    draw(cx);
    press("escape", cx);
    let line =
        |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.commit_line_input());

    press(":", cx);
    let input = line(cx).expect("the line is open");
    caret_keys(&input, cx);
    assert!(line(cx).is_some(), "the line stays open");
    press("escape", cx);
    assert!(line(cx).is_none());

    press(":", cx);
    type_text("abc", cx);
    press("left", cx);
    assert!(line(cx).is_some());
    press("enter", cx);
    assert!(line(cx).is_none());

    press(":", cx);
    press("backspace", cx);
    assert!(line(cx).is_none());
}
