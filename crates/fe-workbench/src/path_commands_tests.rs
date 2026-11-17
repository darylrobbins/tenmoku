//! The active file's path commands: `workbench.copy_active_file_path` and
//! `workbench.copy_active_file_relative_path` put the active editor's path
//! on the clipboard, `workbench.copy_reference` its path and line, and `workbench.reveal_active_file_in_os` hands it to
//! the reveal seam, which records it as the test platform reveals nothing.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::commands::RunCommand;
use gpui::{App, ClipboardItem, Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Direction, Launch, Workbench, path_commands::RevealSeam, tests::open};

static REVEALED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn record(path: &Path, _: &mut App) {
    REVEALED.lock().unwrap().push(path.to_owned());
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on a folder holding `a.txt` and `sub/b.txt`, with `a.txt`
/// open when `file` is set, and the clipboard holding `sentinel`.
fn opened(
    cx: &mut TestAppContext,
    file: bool,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    fs::write(dir.path().join("sub/b.txt"), "b\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: file.then(|| {
            let path = dir.path().join("a.txt");
            (path, Some(Buffer::new("a\n").into()))
        }),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| {
        window.focus(&workbench.focus_handle(cx), cx);
        cx.set_global(RevealSeam(record));
        cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
    });
    draw(cx);
    (dir, workbench, cx)
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn open_file(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

#[gpui::test]
fn copy_path(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, true);
    run("workbench.copy_active_file_path", cx);
    let a = dir.path().join("a.txt");
    assert_eq!(clipboard(cx), Some(a.to_string_lossy().into_owned()));

    open_file(&workbench, dir.path().join("sub/b.txt"), cx);
    run("workbench.copy_active_file_relative_path", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("sub/b.txt"));

    // Outside the folder, the relative path is the absolute one.
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("o.txt");
    fs::write(&path, "o\n").unwrap();
    open_file(&workbench, path.clone(), cx);
    run("workbench.copy_active_file_relative_path", cx);
    assert_eq!(clipboard(cx), Some(path.to_string_lossy().into_owned()));
}

#[gpui::test]
fn copy_leaves_the_clipboard_alone_without_a_file(cx: &mut TestAppContext) {
    // No editor.
    let (_dir, workbench, cx) = opened(cx, false);
    for id in [
        "workbench.copy_active_file_path",
        "workbench.copy_active_file_relative_path",
    ] {
        run(id, cx);
        assert_eq!(clipboard(cx).as_deref(), Some("sentinel"), "{id}");
    }
    // An untitled buffer.
    new_file(&workbench, cx);
    draw(cx);
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_some()));
    for id in [
        "workbench.copy_active_file_path",
        "workbench.copy_active_file_relative_path",
    ] {
        run(id, cx);
        assert_eq!(clipboard(cx).as_deref(), Some("sentinel"), "{id}");
    }
}

#[gpui::test]
fn copy_follows_the_focused_pane(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, true);
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("sub/b.txt"));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.split(Direction::Right, window, cx);
        });
    });
    draw(cx);
    open_file(&workbench, b.clone(), cx);
    run("workbench.copy_active_file_path", cx);
    assert_eq!(clipboard(cx), Some(b.to_string_lossy().into_owned()));

    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.focus_pane(Direction::Left, window, cx);
        });
    });
    draw(cx);
    run("workbench.copy_active_file_path", cx);
    assert_eq!(clipboard(cx), Some(a.to_string_lossy().into_owned()));

    // A rename in the tree follows into the copied path.
    let renamed = dir.path().join("renamed.txt");
    fs::rename(&a, &renamed).unwrap();
    workbench.update(cx, |workbench, cx| {
        workbench.follow_rename(&a, &renamed, cx)
    });
    draw(cx);
    run("workbench.copy_active_file_path", cx);
    assert_eq!(clipboard(cx), Some(renamed.to_string_lossy().into_owned()));
}

/// Selects `range` in the active editor, the caret at its end.
fn select(
    workbench: &Entity<Workbench>,
    range: std::ops::Range<usize>,
    cx: &mut VisualTestContext,
) {
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().clone()
    });
    editor.update(cx, |view, cx| {
        view.model_mut().select_range(range, false);
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn copy_reference_copies_path_and_line(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, false);
    // No file: the clipboard stays.
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("sentinel"));

    let path = dir.path().join("sub/c.txt");
    fs::write(&path, "one\ntwo\nthree\n").unwrap();
    open_file(&workbench, path, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("sub/c.txt:1"));

    // The caret's line; a selection within one line gives that line.
    select(&workbench, 9..9, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("sub/c.txt:3"));
    select(&workbench, 4..6, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("sub/c.txt:2"));

    // Outside the folder, the path is absolute.
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("o.txt");
    fs::write(&path, "o\n").unwrap();
    open_file(&workbench, path.clone(), cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx), Some(format!("{}:1", path.display())));

    // An untitled buffer has none.
    cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
    new_file(&workbench, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("sentinel"));
}

#[gpui::test]
fn copy_reference_gives_a_row_range_for_a_multi_row_selection(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, false);
    let path = dir.path().join("c.txt");
    fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
    open_file(&workbench, path, cx);

    // From `ne` on line 1 to `th` on line 3.
    select(&workbench, 1..10, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("c.txt:1-3"));

    // Whole lines 2 and 3, ending at line 4's start: 4 isn't in it.
    select(&workbench, 4..14, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("c.txt:2-3"));

    // Reversed, the same rows.
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().clone()
    });
    let select_reversed = |range, cx: &mut VisualTestContext| {
        editor.update(cx, |view, cx| {
            view.model_mut().select_range(range, true);
            cx.notify();
        });
        draw(cx);
    };
    select_reversed(4..10, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("c.txt:2-3"));

    // Line 1 whole, ending at line 2's start, either way: line 1 alone.
    select(&workbench, 0..4, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("c.txt:1"));
    select_reversed(0..4, cx);
    run("workbench.copy_reference", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("c.txt:1"));
}

#[gpui::test]
fn reveal(cx: &mut TestAppContext) {
    REVEALED.lock().unwrap().clear();
    let (dir, workbench, cx) = opened(cx, false);
    run("workbench.reveal_active_file_in_os", cx);
    assert_eq!(*REVEALED.lock().unwrap(), Vec::<PathBuf>::new());

    open_file(&workbench, dir.path().join("a.txt"), cx);
    run("workbench.reveal_active_file_in_os", cx);
    assert_eq!(*REVEALED.lock().unwrap(), [dir.path().join("a.txt")]);

    // An untitled buffer has nothing to reveal.
    new_file(&workbench, cx);
    draw(cx);
    run("workbench.reveal_active_file_in_os", cx);
    assert_eq!(REVEALED.lock().unwrap().len(), 1);
}

/// An untitled tab in the active pane, as File > New File opens.
fn new_file(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new(""), None), cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        });
    });
    draw(cx);
}

/// `secondary-k p` and `secondary-k r` through GPUI key input, with the
/// active editor focused, reach the copy and reveal commands.
#[gpui::test]
fn chords_drive_copy_and_reveal(cx: &mut TestAppContext) {
    REVEALED.lock().unwrap().clear();
    let (dir, workbench, cx) = opened(cx, true);
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().clone()
    });
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    // `secondary` is cmd on macOS and ctrl elsewhere.
    let (k, source) = if cfg!(target_os = "macos") {
        ("cmd-k", fe_keymap::DEFAULT_MACOS)
    } else {
        ("ctrl-k", fe_keymap::DEFAULT_LINUX)
    };
    let keymap = fe_keymap::load_keymap(source, None).0;
    cx.update(|_, cx| fe_gpui::set_user_keymap(cx, keymap));

    cx.simulate_keystrokes(&format!("{k} p"));
    draw(cx);
    let a = dir.path().join("a.txt");
    assert_eq!(clipboard(cx), Some(a.to_string_lossy().into_owned()));

    cx.simulate_keystrokes(&format!("{k} r"));
    draw(cx);
    assert_eq!(*REVEALED.lock().unwrap(), [a]);
}
