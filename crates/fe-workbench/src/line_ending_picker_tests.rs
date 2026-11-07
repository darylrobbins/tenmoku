//! Change End of Line Sequence…, from its command and from the status
//! bar's LF/CRLF item.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::LineEnding;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::ItemHandle as _;

use crate::{Launch, Workbench};

/// `files` (name, bytes) in a temporary directory, and their paths.
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

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench rooted at `dir` showing `path`, focused.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    path: &Path,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown");
    (workbench, editor, cx)
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The picker showing: its rows' labels, each with whether it's marked.
fn picker(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Vec<(&'static str, bool)>> {
    let picker = workbench.read_with(cx, |workbench, cx| workbench.line_ending_picker(cx))?;
    Some(picker.read_with(cx, |picker, _| picker.delegate.labels()))
}

/// Types `query` into the picker showing and picks the first match.
fn pick(query: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(query);
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

#[gpui::test]
fn picker_lists_lf_and_crlf_with_the_current_marked(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("win.txt", b"a\r\nb\r\n"), ("unix.txt", b"a\nb\n")]);
    let (workbench, _editor, cx) = opened(cx, &dir, &paths[0]);
    dispatch("workbench.change_line_ending", cx);
    assert_eq!(
        picker(&workbench, cx).expect("the picker shows"),
        [("LF", false), ("CRLF", true)]
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(picker(&workbench, cx).is_none());

    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let unix = paths[1].clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(unix, window, cx)));
    draw(cx);
    dispatch("workbench.change_line_ending", cx);
    assert_eq!(
        picker(&workbench, cx).expect("the picker shows"),
        [("LF", true), ("CRLF", false)]
    );
}

#[gpui::test]
fn picking_converts_and_marks_the_buffer_modified(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("win.txt", b"a\r\nb\r\n")]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let ending = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (
                editor.model().buffer().line_ending(),
                editor.model().buffer().is_dirty(),
            )
        })
    };
    assert_eq!(ending(cx), (LineEnding::CrLf, false));

    dispatch("workbench.change_line_ending", cx);
    pick("lf", cx);
    assert!(picker(&workbench, cx).is_none());
    assert_eq!(ending(cx), (LineEnding::Lf, true));

    let save = cx.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)));
    save.detach();
    draw(cx);
    assert_eq!(fs::read(&paths[0]).unwrap(), b"a\nb\n");
    assert_eq!(ending(cx), (LineEnding::Lf, false));

    // Undo restores CRLF, which now differs from the saved LF file.
    editor.update(cx, |editor, _| {
        editor.model_mut().undo().unwrap();
    });
    assert_eq!(ending(cx), (LineEnding::CrLf, true));
}

#[gpui::test]
fn the_status_item_runs_the_command(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("win.txt", b"a\r\nb\r\n")]);
    let (_workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let item = |cx: &mut VisualTestContext| {
        let items = cx.update(|_, cx| editor.status_items(cx));
        items
            .into_iter()
            .find(|item| item.id == "line_ending")
            .expect("the line ending item")
    };
    let item_now = item(cx);
    assert_eq!(item_now.text, "CRLF");
    assert_eq!(
        item_now.command.unwrap().command.as_str(),
        "workbench.change_line_ending"
    );
    assert_eq!(
        item_now.tooltip.as_deref(),
        Some("Select End of Line Sequence")
    );

    editor.update(cx, |editor, cx| editor.set_read_only(true, cx));
    assert!(item(cx).command.is_none());
}
