//! A viewer tab's Edit This File converts it to an editor in place: the
//! file is read off the main thread, and refused if it changed on disk,
//! before or during the read. Above the (lowered) warning threshold it asks
//! first.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use fe_gpui::{FileViewer, FileViewerEvent};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{Pane, lower_edit_warning};
use crate::{Launch, Workbench, loader::test_support};

/// The lowered large-file threshold.
const THRESHOLD: u64 = 64 * 1024;

fn line(row: usize) -> String {
    format!("line {row:06}\n")
}

fn text() -> String {
    (0..(THRESHOLD as usize / line(0).len() + 100))
        .map(line)
        .collect()
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A project with one large file, `big.txt`, open in a viewer tab after a
/// small one, `small.txt`.
fn open_viewer(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Pane>,
    Entity<FileViewer>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    test_support::lower_threshold(&root, THRESHOLD, false);
    let big = root.join("big.txt");
    std::fs::write(&big, text()).unwrap();
    std::fs::write(root.join("small.txt"), "small\n").unwrap();
    std::fs::write(root.join("other.txt"), "other\n").unwrap();
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench: &Workbench, _| workbench.pane().clone());
    for name in ["small.txt", "big.txt", "other.txt"] {
        let path = root.join(name);
        pane.update_in(cx, |pane, window, cx| pane.open(path, window, cx));
        draw(cx);
    }
    // Show the viewer, in the middle.
    pane.update_in(cx, |pane, window, cx| pane.activate(1, window, cx));
    draw(cx);
    let viewer = pane
        .read_with(cx, |pane, _| pane.tabs()[1].viewer().cloned())
        .expect("a viewer is open at tab 1");
    (dir, big, pane, viewer, cx)
}

fn request(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) {
    viewer.update(cx, |_, cx| cx.emit(FileViewerEvent::EditRequested));
    cx.run_until_parked();
}

fn banner(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> String {
    viewer.read_with(cx, |viewer, _| viewer.banner().to_string())
}

fn is_viewer(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| pane.tabs()[ix].viewer().is_some())
}

fn append(path: &Path) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(b"more\n").unwrap();
}

#[gpui::test]
fn edit_this_file_converts_in_place(cx: &mut TestAppContext) {
    let (_dir, big, pane, viewer, cx) = open_viewer(cx);
    // Wait for the index, which builds on a thread of its own, to find rows.
    for _ in 0..200 {
        if viewer.read_with(cx, |viewer, _| viewer.model().text().row_count()) > 5 {
            break;
        }
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        draw(cx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let head = viewer.read_with(cx, |viewer, _| viewer.model().text().row_start(3) + 2);
    let state = fe_command::Value::from_iter([("head", fe_command::Value::from(head))]);
    pane.update_in(cx, |pane, window, cx| {
        pane.tabs()[1].item().apply_state(&state, window, cx)
    });
    draw(cx);
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.model().head()),
        head
    );
    assert!(head > 2);
    request(&viewer, cx);
    draw(cx);
    pane.read_with(cx, |pane, cx| {
        assert_eq!(pane.tabs().len(), 3, "replaced in place");
        assert_eq!(pane.active(), Some(1));
        let tab = &pane.tabs()[1];
        assert!(tab.viewer().is_none());
        assert_eq!(tab.path.as_deref(), Some(big.as_path()));
        let model = tab.editor().expect("an editor").read(cx).model();
        assert_eq!(model.path(), Some(big.as_path()));
        assert_eq!(model.text().to_string(), text());
        assert_eq!(model.primary_selection().head(), head);
    });
}

#[gpui::test]
fn changed_files_are_refused(cx: &mut TestAppContext) {
    // Before the request.
    let (_dir, big, pane, viewer, cx) = open_viewer(cx);
    append(&big);
    request(&viewer, cx);
    assert!(is_viewer(&pane, 1, cx));
    assert!(
        banner(&viewer, cx).contains("changed on disk"),
        "{}",
        banner(&viewer, cx)
    );
    assert!(viewer.read_with(cx, |viewer, _| viewer.offers_reload()));

    // While it's read.
    let (_dir, big, pane, viewer, cx) = open_viewer(cx);
    viewer.update(cx, |_, cx| cx.emit(FileViewerEvent::EditRequested));
    // Step until the conversion is under way, then change the file.
    for _ in 0..100 {
        if viewer.read_with(cx, |viewer, _| viewer.is_converting()) {
            break;
        }
        cx.executor().tick();
    }
    assert!(viewer.read_with(cx, |viewer, _| viewer.is_converting()));
    append(&big);
    cx.run_until_parked();
    assert!(is_viewer(&pane, 1, cx));
    assert!(
        banner(&viewer, cx).contains("changed on disk"),
        "{}",
        banner(&viewer, cx)
    );
}

#[gpui::test]
fn large_conversions_ask_first(cx: &mut TestAppContext) {
    let (dir, _big, pane, viewer, cx) = open_viewer(cx);
    let root = dunce::canonicalize(dir.path()).unwrap();
    lower_edit_warning(&root, THRESHOLD);
    request(&viewer, cx);
    assert!(cx.has_pending_prompt());
    let (question, _) = cx.pending_prompt().unwrap();
    assert_eq!(question, "Edit big.txt?");
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(is_viewer(&pane, 1, cx));
    assert!(!viewer.read_with(cx, |viewer, _| viewer.is_converting()));
    request(&viewer, cx);
    cx.simulate_prompt_answer("Edit");
    draw(cx);
    assert!(!is_viewer(&pane, 1, cx));
    assert_eq!(pane.read_with(cx, |pane, _| pane.tabs().len()), 3);
}

/// A reveal in a viewer whose index is held back lands once the index
/// reaches it, by byte offset and by row.
#[gpui::test]
fn viewer_reveals_wait_for_the_index(cx: &mut TestAppContext) {
    use super::Reveal;
    let big_text: String = (0..(2 * fe_fileview::CHUNK_BYTES as usize) / line(0).len() + 100)
        .map(line)
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    test_support::lower_threshold(&root, THRESHOLD, true);
    for name in ["bytes.txt", "rows.txt"] {
        std::fs::write(root.join(name), &big_text).unwrap();
    }
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench: &Workbench, _| workbench.pane().clone());
    let last_row = big_text.lines().count() - 20;
    let byte_target = last_row * line(0).len() + 3;
    let reveals = [
        ("bytes.txt", Reveal::bytes(byte_target..byte_target + 1)),
        (
            "rows.txt",
            Reveal {
                row: last_row as u32,
                columns: 3..4,
                within_row: true,
            },
        ),
    ];
    for (name, reveal) in reveals {
        let path = root.join(name);
        pane.update_in(cx, |pane, window, cx| {
            pane.open_at(path.clone(), Some(reveal), window, cx)
        });
        draw(cx);
        let viewer = pane
            .read_with(cx, |pane, _| {
                pane.tabs()[pane.active().unwrap()].viewer().cloned()
            })
            .expect("a viewer");
        // One chunk is indexed, which the target lies beyond.
        let mut steps = test_support::take_steps(&path);
        assert_eq!(steps.len(), 1);
        let mut step = steps.pop().unwrap();
        step.next().expect("building");
        cx.executor().advance_clock(Duration::from_millis(200));
        draw(cx);
        assert!(viewer.read_with(cx, |viewer, _| viewer.model().head()) < byte_target);
        // Unheld, the index finishes and the reveal lands.
        drop(step);
        for _ in 0..500 {
            cx.executor().advance_clock(Duration::from_millis(200));
            draw(cx);
            if viewer.read_with(cx, |viewer, _| viewer.model().head()) == byte_target {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            viewer.read_with(cx, |viewer, _| viewer.model().head()),
            byte_target,
            "{name}"
        );
    }
}

/// A conversion that outlives its viewer's place: moved to another pane,
/// and a newer open and the window's focus meanwhile.
#[gpui::test]
fn conversions_leave_other_state_alone(cx: &mut TestAppContext) {
    let (_dir, _big, pane, viewer, cx) = open_viewer(cx);
    viewer.update(cx, |_, cx| cx.emit(FileViewerEvent::EditRequested));
    for _ in 0..100 {
        if viewer.read_with(cx, |viewer, _| viewer.is_converting()) {
            break;
        }
        cx.executor().tick();
    }
    // A newer open is pending, and another tab has the focus.
    pane.update(cx, |pane, cx| {
        pane.opening(PathBuf::from("/nowhere/else.txt"), cx)
    });
    cx.run_until_parked();
    pane.read_with(cx, |pane, _| {
        assert!(pane.tabs()[1].viewer().is_none(), "converted");
        assert!(pane.opening.is_some(), "the newer open is still pending");
    });
}
