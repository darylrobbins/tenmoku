//! A language picked in Change Language Mode is remembered for the file:
//! closed and opened again, it opens in that language, chosen. Auto Detect,
//! the picker's first row, forgets it and detects the language again; a
//! remembered language no longer there leaves it to detection.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{Workbench, language_picker};

/// `files` (name, contents) in a temporary directory, and their paths.
fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = files
        .iter()
        .map(|(name, bytes)| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

/// A workbench rooted at `dir`, focused, keeping the lists under `state`.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    state: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| language_picker::set_state_dir(cx, state.to_owned()));
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Opens `path` in the active pane, focused, and gives its editor.
fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

/// Closes the active tab.
fn close(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active(window, cx)));
    draw(cx);
    let open = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_some());
    assert!(!open, "the tab closed");
}

/// Picks the first row of Change Language Mode matching `query`.
fn pick(query: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of("language.select")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    if !query.is_empty() {
        cx.simulate_input(query);
        draw(cx);
    }
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// The editor's language, `None` for plain text, and whether it was chosen.
fn language(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Option<String>, bool) {
    editor.read_with(cx, |editor, _| {
        let name = editor
            .syntax()
            .map(|syntax| syntax.language().name().to_owned());
        (name, editor.is_language_chosen())
    })
}

#[gpui::test]
fn a_picked_language_is_restored_and_auto_detect_forgets_it(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (dir, paths) = project(&[("notes.zz", b"{\"a\": 1}\n")]);
    let (workbench, cx) = opened(cx, &dir, state.path());
    let json = (Some("json".to_owned()), false);

    // An unknown extension goes by the content.
    let editor = open(&workbench, &paths[0], cx);
    assert_eq!(language(&editor, cx), json);

    pick("python", cx);
    assert_eq!(language(&editor, cx), (Some("python".into()), true));
    close(&workbench, cx);
    let editor = open(&workbench, &paths[0], cx);
    assert_eq!(language(&editor, cx), (Some("python".into()), true));

    // Plain Text is remembered too.
    pick("plain text", cx);
    assert_eq!(language(&editor, cx), (None, true));
    close(&workbench, cx);
    let editor = open(&workbench, &paths[0], cx);
    assert_eq!(language(&editor, cx), (None, true));

    // Auto Detect, the first row, detects it again and forgets the pick.
    pick("", cx);
    assert_eq!(language(&editor, cx), json);
    close(&workbench, cx);
    let editor = open(&workbench, &paths[0], cx);
    assert_eq!(language(&editor, cx), json);
    let remembered = fe_config::language_choices::remembered(state.path(), &paths[0]);
    assert_eq!(remembered, None);
}

#[gpui::test]
fn a_remembered_language_no_longer_there_is_detected(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (dir, paths) = project(&[("notes.zz", b"<a><b/></a>\n")]);
    fe_config::language_choices::remember(
        state.path(),
        Some(dir.path()),
        &paths[0],
        Some("no-such-language"),
    )
    .unwrap();
    let (workbench, cx) = opened(cx, &dir, state.path());

    let editor = open(&workbench, &paths[0], cx);
    assert_eq!(language(&editor, cx), (Some("xml".into()), false));
}
