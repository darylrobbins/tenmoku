//! A file link's `#line,col` landing on its column: in a viewer, for
//! overlapping opens in two panes, behind a Save As and when the column is
//! oversized.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, FileViewer, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench, loader::test_support, pane::Pane, tests::open};

/// The lowered large-file threshold.
const THRESHOLD: u64 = 64 * 1024;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A project of `files`, a workbench on it, and the folder.
fn project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, root, workbench, cx)
}

/// Follows a link to `path` at `fragment` in the active pane.
fn follow(workbench: &Entity<Workbench>, path: &Path, fragment: &str, cx: &mut VisualTestContext) {
    let path = path.to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_link_file(path, Some(fragment), window, cx)
        })
    });
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

/// Draws until `done`, a viewer's index being polled on the clock.
fn until(what: &str, cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.executor().advance_clock(Duration::from_millis(200));
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The editor of `pane`'s tab for `path`, and its caret.
fn caret(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) -> Option<usize> {
    pane.read_with(cx, |pane, cx| {
        let editor: &Entity<EditorView> = (pane.tabs().iter())
            .filter_map(|tab| tab.editor())
            .find(|editor| editor.read(cx).model().path() == Some(path))?;
        Some(editor.read(cx).model().primary_selection().head())
    })
}

#[gpui::test]
fn link_column_lands_in_viewer(cx: &mut TestAppContext) {
    // `é` is one unit and two bytes, `😀` two units and four: the `y` is
    // the 7th unit and the 9th byte of the last row, far past the first
    // chunk of the index.
    let rows = (2 * fe_fileview::CHUNK_BYTES as usize) / 12 + 100;
    let mut text: String = (0..rows).map(|row| format!("line {row:06}\n")).collect();
    let last_row = rows as u32;
    let start = text.len();
    text.push_str("é x 😀y\n");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text)]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, true);

    follow(&workbench, &path, &format!("L{},7", last_row + 1), cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let viewer: Entity<FileViewer> = pane
        .read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
        .expect("a viewer is shown");
    // Its index holds, the row not found yet; then it finishes.
    drop(test_support::take_steps(&path));
    until("the caret on the `y`", cx, |cx| {
        viewer.read_with(cx, |viewer, _| {
            viewer.model().selection() == (start + 9..start + 9)
        })
    });
}

#[gpui::test]
fn overlapping_opens_keep_their_landings(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(
        cx,
        &[("a.rs", "ab\ncd\n"), ("b.rs", "xy\nzw\n"), ("c.rs", "\n")],
    );
    follow(&workbench, &root.join("c.rs"), "L1", cx);
    draw(cx);
    run("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    assert_eq!(panes.len(), 2);
    let (first, second) = (panes[0].clone(), panes[1].clone());

    // Both opens start before either file is read.
    for pane in [&first, &second] {
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.activate(pane.clone(), window, cx)
            })
        });
        let name = if *pane == first { "a.rs" } else { "b.rs" };
        follow(&workbench, &root.join(name), "L2,2", cx);
    }
    draw(cx);
    // The `d`, and the `w`.
    assert_eq!(caret(&first, &root.join("a.rs"), cx), Some(4));
    assert_eq!(caret(&second, &root.join("b.rs"), cx), Some(4));
}

#[gpui::test]
fn link_survives_save_as_reservation(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "ab\ncd\n")]);
    let path = root.join("a.rs");
    let saving = crate::open::reserve(
        fe_buffer::Buffer::new("").id(),
        path.clone(),
        crate::open::file_identity(&path),
    );
    follow(&workbench, &path, "L2,2", cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(caret(&pane, &path, cx), None, "waiting behind the Save As");
    drop(saving);
    draw(cx);
    assert_eq!(caret(&pane, &path, cx), Some(4));
}

#[gpui::test]
fn oversized_fragment_column_is_clamped(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "abc\ndef\n")]);
    let path = root.join("a.rs");
    // 2^32 + 2: truncated to 32 bits it would be the column of the `b`.
    follow(&workbench, &path, "L1,4294967298", cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(caret(&pane, &path, cx), Some(3), "the end of the row");
    // Past a `u64`'s digits as well.
    follow(&workbench, &path, "L2,99999999999999999999999", cx);
    draw(cx);
    assert_eq!(caret(&pane, &path, cx), Some(7));
}

/// A large project file of `rows` rows `line 000000`, then `tail`.
fn large_text(tail: &str) -> (String, usize, u32) {
    let rows = (2 * fe_fileview::CHUNK_BYTES as usize) / 12 + 100;
    let mut text: String = (0..rows).map(|row| format!("line {row:06}\n")).collect();
    let start = text.len();
    text.push_str(tail);
    (text, start, rows as u32)
}

#[gpui::test]
fn viewer_waits_out_the_placeholder_row(cx: &mut TestAppContext) {
    let (text, _, _) = large_text("");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text)]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, true);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    draw(cx);
    let viewer: Entity<FileViewer> = pane
        .read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
        .expect("a viewer is shown");
    // One chunk indexed: the last row it has is a placeholder.
    let mut steps = test_support::take_steps(&path);
    let mut step = steps.pop().expect("a held index");
    step.next().expect("the index is building");
    cx.executor().advance_clock(Duration::from_millis(200));
    draw(cx);
    let rows = viewer.read_with(cx, |viewer, _| viewer.model().text().row_count());
    assert!(viewer.read_with(cx, |viewer, _| viewer.is_polling()));

    // Its column is 6: five units along `line 0000nn`.
    follow(&workbench, &path, &format!("L{rows},6"), cx);
    draw(cx);
    drop(step);
    until("the caret five units along", cx, |cx| {
        viewer.read_with(cx, |viewer, _| {
            let text = viewer.model().text();
            let at = viewer.model().selection();
            !viewer.is_polling()
                && text.row_count() > rows
                && at == (text.row_start(rows - 1) + 5..text.row_start(rows - 1) + 5)
        })
    });
}

#[gpui::test]
fn a_later_open_of_the_same_file_cancels_the_landing(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "ab\ncd\n")]);
    let path = root.join("a.rs");
    // The second open supersedes the first while the file is read.
    follow(&workbench, &path, "L2,2", cx);
    follow(&workbench, &path, "L1", cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(caret(&pane, &path, cx), Some(0));
}

#[gpui::test]
fn a_queued_viewer_landing_checks_the_open_again(cx: &mut TestAppContext) {
    let (text, _, _) = large_text("");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text), ("b.rs", "b\n")]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, false);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    let viewer: Entity<FileViewer> = {
        draw(cx);
        pane.read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
            .expect("a viewer is shown")
    };
    until("the index", cx, |cx| {
        viewer.read_with(cx, |viewer, _| !viewer.is_polling())
    });

    // The landing queues its reopen; another open comes before it runs.
    let other = root.join("b.rs");
    cx.update(|window, cx| {
        let handle = window.window_handle();
        workbench.update(cx, |workbench, cx| {
            workbench.land_link_column(&pane, handle, path.clone(), (0, 5), false, cx)
        });
        pane.update(cx, |pane, cx| pane.open(other.clone(), window, cx));
    });
    draw(cx);
    let active = pane.read_with(cx, |pane, _| {
        pane.tabs()[pane.active().unwrap()].path.clone()
    });
    assert_eq!(
        active,
        Some(other),
        "the stale landing doesn't take the tab back"
    );
}
