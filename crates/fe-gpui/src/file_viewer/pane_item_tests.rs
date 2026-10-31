//! A [`FileViewer`] as a pane item, through kiln-shell's item handle, and
//! its file changing on disk under it: the banner's Reload, and a reload's
//! new rows. Over generated files in a headless window.

use std::{
    cell::RefCell,
    fs::OpenOptions,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_command::Value;
use fe_fileview::{FileView, IndexState, LineIndex};
use gpui::{
    AppContext as _, Entity, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
    VisualTestContext, point, px,
};
use kiln_shell::{CloseAnswer, ItemHandle, PaneItem as _};
use tempfile::TempDir;

use super::*;
use crate::{EditorTheme, pane_item::status};

const BANNER: &str = "Read only: too large to edit";

/// The text of a file of `rows` rows, each 63 bytes and a `\n` but the
/// last, which starts with `prefix`, and its rows.
fn rows_text(prefix: &str, rows: usize) -> (String, Vec<String>) {
    let lines: Vec<String> = (0..rows)
        .map(|row| {
            let mut line = format!(
                "{prefix} {row:08} {}",
                "abcdefghijklmnopqrstuvwxyz".repeat(3)
            );
            line.truncate(63);
            line
        })
        .collect();
    (lines.join("\n"), lines)
}

/// `big.log` in a new tempdir, of `rows_text(prefix, rows)`.
fn generated(prefix: &str, rows: usize) -> (TempDir, PathBuf, String, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.log");
    let (text, lines) = rows_text(prefix, rows);
    std::fs::write(&path, &text).unwrap();
    (dir, path, text, lines)
}

type Events = Rc<RefCell<Vec<FileViewerEvent>>>;

/// The window's root, which makes the viewer and subscribes to it at once,
/// as a pane would.
struct Host {
    viewer: Entity<FileViewer>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.viewer.clone()
    }
}

/// A focused viewer of `path`, its index complete, in a headless window,
/// and the events it sends.
fn viewer<'a>(
    cx: &'a mut TestAppContext,
    path: &Path,
) -> (Entity<FileViewer>, &'a mut VisualTestContext, Events) {
    let view = Arc::new(FileView::open(path).unwrap());
    let index = complete_index(&view);
    viewer_of(cx, view, index, false)
}

/// A whole index of `view`, built a chunk at a time on this thread.
fn complete_index(view: &Arc<FileView>) -> LineIndex {
    let (index, step) = LineIndex::build_stepped(view.clone());
    step.for_each(drop);
    assert_eq!(index.progress().state, IndexState::Complete);
    index
}

/// A viewer of `view` and `index` in a headless window, focused from its
/// first frame if `at_once`, else after it, and the events it sends.
fn viewer_of(
    cx: &mut TestAppContext,
    view: Arc<FileView>,
    index: LineIndex,
    at_once: bool,
) -> (Entity<FileViewer>, &mut VisualTestContext, Events) {
    let events = Events::default();
    let heard = events.clone();
    let (host, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let viewer = cx.new(|cx| FileViewer::new(view, Arc::new(index), BANNER.into(), theme, cx));
        if at_once {
            window.focus(&viewer.focus_handle(cx), cx);
        }
        cx.subscribe(&viewer, move |_, _, event: &FileViewerEvent, _| {
            heard.borrow_mut().push(event.clone())
        })
        .detach();
        Host { viewer }
    });
    let viewer = host.read_with(cx, |host, _| host.viewer.clone());
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.run_until_parked();
    (viewer, cx, events)
}

/// Runs `viewer`'s polls until its index, built on its own thread, is
/// done and it has taken all of it.
fn settle(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) {
    for _ in 0..1000 {
        cx.executor().advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
        if viewer.read_with(cx, |v, _| !v.is_polling() && !v.is_reloading()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the index never finished");
}

/// Adds to the end of `path`: a change no read within the old length
/// sees, so only a check finds it.
fn append(path: &Path) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    std::io::Write::write_all(&mut file, b"\nmore").unwrap();
}

/// The banner as last drawn, whether it offers a reload, and whether the
/// button is drawn.
fn banner(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> (String, bool, bool) {
    let (text, drawn_text, offers) = viewer.read_with(cx, |v, _| {
        (
            v.banner().to_string(),
            v.drawn_banner.to_string(),
            v.offers_reload(),
        )
    });
    assert_eq!(drawn_text, text, "the banner is drawn as it stands");
    let drawn = cx.debug_bounds("file-viewer-reload").is_some();
    (drawn_text, offers, drawn)
}

/// The first row painted wholly below the banner: its row and text.
fn top_row(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> (u32, String) {
    viewer.read_with(cx, |viewer, _| {
        let top = viewer.layout.as_ref().expect("drawn").origin.y;
        let (row, text, _) = viewer
            .painted_rows
            .iter()
            .find(|(_, _, origin)| origin.y >= top)
            .expect("rows painted");
        (*row, text.clone())
    })
}

/// `state` restored, once the task is done.
fn restore(cx: &mut VisualTestContext, state: Value) -> Option<Entity<FileViewer>> {
    let task = cx.update(|window, cx| FileViewer::restore(state, window, cx));
    let result = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let result = result.clone();
        cx.spawn(async move |_| *result.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    result.take().expect("done")
}

#[gpui::test]
fn is_a_read_only_pane_item(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, _, lines) = generated("row", 300);
    let (_other_dir, other_path, ..) = generated("other", 10);
    let (viewer, cx, events) = viewer(cx, &path);
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    assert_eq!(item.kind(), "fastedit.file_viewer");
    cx.update(|window, cx| {
        assert_eq!(item.tab_title(cx), "big.log");
        assert_eq!(item.window_title(cx), "big.log — Tenmoku");
        assert_eq!(item.path(cx), Some(path.clone()));
        assert!(!item.is_dirty(cx));
        assert!(item.save(window, cx).is_none(), "nothing to save");
        assert!(matches!(item.may_close(window, cx), CloseAnswer::Close));
    });

    // Row 3, col 8, with "ijk" selected back to col 5.
    let row = 2 * 64;
    viewer.update(cx, |viewer, cx| {
        viewer.model.set_selection(row + 7, row + 4);
        cx.notify();
    });
    cx.run_until_parked();
    let items = cx.update(|_, cx| item.status_items(cx));
    let items: Vec<_> = items
        .iter()
        .map(|item| (item.id.to_string(), item.text.to_string()))
        .collect();
    assert_eq!(
        items,
        [
            ("read_only".into(), "Read-only".into()),
            (status::CURSOR.into(), "Ln 3, Col 5 (3 selected)".into()),
        ]
    );
    viewer.update(cx, |viewer, cx| {
        viewer.model.set_selection(row + 7, row + 7);
        cx.notify();
    });
    cx.run_until_parked();
    let cursor = cx.update(|_, cx| item.status_items(cx)[1].text.clone());
    assert_eq!(cursor, "Ln 3, Col 8");

    // Still the viewer's own: dirty never, and closing never asks.
    cx.simulate_keystrokes("x");
    cx.update(|window, cx| {
        assert!(!item.is_dirty(cx));
        assert!(matches!(item.may_close(window, cx), CloseAnswer::Close));
    });
    assert_eq!(*events.borrow(), [FileViewerEvent::ReadOnly]);

    viewer.update(cx, |viewer, cx| {
        viewer.model.set_selection(row + 4, row + 7);
        cx.notify();
    });
    let split = cx
        .update(|window, cx| item.split(window, cx))
        .expect("splits");
    let other: Box<dyn ItemHandle> = Box::new(cx.update(|_, cx| {
        let view = Arc::new(FileView::open(&other_path).unwrap());
        let index = Arc::new(LineIndex::build(view.clone()));
        let theme = EditorTheme::default_dark(cx);
        cx.new(|cx| FileViewer::new(view, index, BANNER.into(), theme, cx))
    }));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(item.shares_document(&*split, cx));
        assert!(split.shares_document(&*item, cx));
        assert!(!item.shares_document(&*other, cx), "another file");
        assert_eq!(split.path(cx), Some(path.clone()));
    });
    let split = split.downcast::<FileViewer>().expect("a viewer");
    split.read_with(cx, |split, cx| {
        let original = viewer.read(cx);
        assert!(Arc::ptr_eq(&split.view, &original.view));
        assert!(Arc::ptr_eq(&split.index, &original.index));
        assert_eq!(split.model().selection(), row + 4..row + 7);
        assert_eq!(split.model().head(), row + 7);
        assert_eq!(split.banner(), BANNER);
        assert_eq!(split.model().display().row_count() as usize, lines.len());
    });
}

#[gpui::test]
fn serialize_and_restore(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (dir, path, _, _) = generated("row", 2000);
    let (viewer, cx, _) = viewer(cx, &path);
    let head = 1500 * 64 + 10;
    viewer.update(cx, |viewer, cx| {
        viewer.model.set_selection(head - 20, head);
        cx.notify();
    });
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    let state = cx.update(|_, cx| item.serialize(cx)).expect("saved");
    assert_eq!(state.get("path").and_then(Value::as_str), path.to_str());
    assert_eq!(state.get("head").and_then(Value::as_u64), Some(head as u64));

    let restored = restore(cx, state.clone()).expect("restored");
    settle(&restored, cx);
    restored.read_with(cx, |restored, _| {
        assert_eq!(restored.view.path(), path);
        assert_eq!(restored.model().selection(), head..head);
        assert_eq!(restored.banner(), BANNER);
        assert_eq!(restored.model().display().row_count(), 2000);
    });

    // A saved head past the end is clipped to it.
    let past = Value::from_iter([
        ("path", Value::from(path.to_str().unwrap())),
        ("head", Value::from(u64::MAX)),
    ]);
    let restored = restore(cx, past).expect("restored");
    settle(&restored, cx);
    let len = std::fs::metadata(&path).unwrap().len() as usize;
    restored.read_with(cx, |restored, _| {
        assert_eq!(restored.model().selection(), len..len);
    });

    drop(dir);
    assert!(!path.exists());
    assert!(
        restore(cx, state).is_none(),
        "a deleted file isn't restored"
    );
}

#[gpui::test]
fn changed_on_disk_shows_reload(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let truncate = |path: &Path| {
        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_len(1000).unwrap();
    };
    let rewrite = |path: &Path| std::fs::write(path, rows_text("new", 123).0).unwrap();
    let changes: [&dyn Fn(&Path); 2] = [&truncate, &rewrite];
    for change in changes {
        let (_dir, path, _, _) = generated("row", 300);
        let (viewer, cx, events) = viewer(cx, &path);
        assert_eq!(banner(&viewer, cx), (BANNER.into(), false, false));

        cx.executor().advance_clock(CHECK_INTERVAL);
        cx.run_until_parked();
        assert_eq!(*events.borrow(), [], "unchanged");

        change(&path);
        cx.executor()
            .advance_clock(CHECK_INTERVAL - Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(*events.borrow(), [], "not checked yet");
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
        assert_eq!(
            banner(&viewer, cx),
            ("big.log changed on disk.".into(), true, true)
        );

        cx.executor().advance_clock(CHECK_INTERVAL * 3);
        cx.run_until_parked();
        assert_eq!(*events.borrow(), [FileViewerEvent::Changed], "once");
    }
}

#[gpui::test]
fn focus_checks_the_file(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, _, _) = generated("row", 300);
    let (viewer, cx, events) = viewer(cx, &path);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();

    // Back to the window: the viewer gets focus again.
    cx.deactivate_window();
    append(&path);
    assert_eq!(*events.borrow(), []);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
    assert!(viewer.read_with(cx, |v, _| v.offers_reload()));

    // Back to the viewer, after a reload.
    viewer.update(cx, |viewer, cx| viewer.reload(cx));
    settle(&viewer, cx);
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    append(&path);
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.run_until_parked();
    assert_eq!(
        *events.borrow(),
        [
            FileViewerEvent::Changed,
            FileViewerEvent::Reloaded,
            FileViewerEvent::Changed
        ]
    );
}

#[gpui::test]
fn reload_swaps_in_the_new_file(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, old, _) = generated("old", 2000);
    let (viewer, cx, events) = viewer(cx, &path);
    let top = viewer.read_with(cx, |v, _| v.layout.as_ref().expect("drawn").origin.y);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(200.), top + px(100.)),
        delta: ScrollDelta::Lines(point(0., -300.)),
        ..Default::default()
    });
    cx.run_until_parked();
    let (row, _) = top_row(&viewer, cx);
    assert!(row >= 250, "scrolled to row {row}");
    // Selected to the end, past the new file's.
    viewer.update(cx, |viewer, cx| {
        viewer.model.set_selection(old.len() - 10, old.len());
        cx.notify();
    });
    cx.run_until_parked();

    let (new, new_lines) = rows_text("new", 1000);
    std::fs::write(&path, &new).unwrap();
    cx.executor().advance_clock(CHECK_INTERVAL);
    cx.run_until_parked();
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);

    let reload = cx
        .debug_bounds("file-viewer-reload")
        .expect("a Reload button");
    cx.simulate_click(reload.center(), Modifiers::none());
    settle(&viewer, cx);
    assert_eq!(
        *events.borrow(),
        [FileViewerEvent::Changed, FileViewerEvent::Reloaded]
    );
    assert_eq!(banner(&viewer, cx), (BANNER.into(), false, false));
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.model().selection(), new.len()..new.len(), "clipped");
        assert_eq!(
            viewer.model().display().row_count() as usize,
            new_lines.len()
        );
    });
    assert_eq!(
        top_row(&viewer, cx),
        (row, new_lines[row as usize].clone()),
        "the scroll row is kept"
    );
    let rows = viewer.read_with(cx, |v, _| v.painted_rows.clone());
    for (row, text, _) in &rows {
        assert_eq!(text, &new_lines[*row as usize]);
    }

    // The reloaded file is watched in turn.
    std::fs::write(&path, rows_text("newer", 10).0).unwrap();
    cx.executor().advance_clock(CHECK_INTERVAL);
    cx.run_until_parked();
    assert_eq!(
        *events.borrow(),
        [
            FileViewerEvent::Changed,
            FileViewerEvent::Reloaded,
            FileViewerEvent::Changed
        ]
    );

    // A reload that can't open the file says so, and offers another.
    std::fs::remove_file(&path).unwrap();
    viewer.update(cx, |viewer, cx| viewer.reload(cx));
    cx.run_until_parked();
    let (text, offers, drawn) = banner(&viewer, cx);
    assert!(text.starts_with("Cannot read big.log: "), "{text}");
    assert!(offers && drawn);
    assert_eq!(events.borrow().len(), 3, "no Reloaded");
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.model().display().row_count() as usize,
            new_lines.len()
        );
    });
}

#[gpui::test]
fn the_cursor_is_worked_out_off_the_main_thread_once_per_move(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, _, _) = generated("row", 2000);
    let (viewer, cx, _) = viewer(cx, &path);
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    let worked_out = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.cursors_worked_out);
    let cursor =
        |cx: &mut VisualTestContext| cx.update(|_, cx| item.status_items(cx)[1].text.clone());
    assert_eq!(cursor(cx), "Ln 1, Col 1");
    let before = worked_out(cx);

    let head = 1500 * 64 + 10;
    viewer.update(cx, |viewer, cx| {
        viewer.model.set_selection(head, head);
        cx.notify();
    });
    // Asking reads nothing: the last text stands until the new one's in.
    assert_eq!(cursor(cx), "Ln 1, Col 1");
    assert_eq!(worked_out(cx), before);
    cx.run_until_parked();
    assert_eq!(cursor(cx), "Ln 1501, Col 11");
    assert_eq!(worked_out(cx), before + 1);

    for _ in 0..50 {
        assert_eq!(cursor(cx), "Ln 1501, Col 11");
        viewer.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    assert_eq!(worked_out(cx), before + 1, "once for the move");
}

#[gpui::test]
fn a_failed_reload_keeps_its_banner(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, _, _) = generated("row", 300);
    let (viewer, cx, events) = viewer(cx, &path);
    // Before any check has found it changed.
    std::fs::remove_file(&path).unwrap();
    viewer.update(cx, |viewer, cx| viewer.reload(cx));
    cx.run_until_parked();
    let (text, offers, drawn) = banner(&viewer, cx);
    assert!(text.starts_with("Cannot read big.log: "), "{text}");
    assert!(offers && drawn);
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);

    cx.executor().advance_clock(CHECK_INTERVAL * 3);
    cx.run_until_parked();
    assert_eq!(
        banner(&viewer, cx),
        (text.clone(), true, true),
        "checks leave it be"
    );
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);

    // A retry that fails otherwise draws its own error.
    std::fs::create_dir(&path).unwrap();
    viewer.update(cx, |viewer, cx| viewer.reload(cx));
    cx.run_until_parked();
    let (retried, offers, drawn) = banner(&viewer, cx);
    assert!(retried.starts_with("Cannot read big.log: "), "{retried}");
    assert_ne!(retried, text);
    assert!(offers && drawn);
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}

#[gpui::test]
fn a_viewer_focused_on_its_first_frame_checks_at_once(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, _, _) = generated("row", 300);
    let view = Arc::new(FileView::open(&path).unwrap());
    let index = complete_index(&view);
    append(&path);
    let (viewer, cx, events) = viewer_of(cx, view, index, true);
    assert_eq!(
        *events.borrow(),
        [FileViewerEvent::Changed],
        "no clock advanced"
    );
    assert!(viewer.read_with(cx, |v, _| v.offers_reload()));
}

#[gpui::test]
fn closing_the_window_stops_the_checks(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_dir, path, _, _) = generated("row", 300);
    let (viewer, cx, _) = viewer(cx, &path);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();

    append(&path);
    cx.executor().advance_clock(CHECK_INTERVAL * 3);
    cx.run_until_parked();
    let viewer = viewer.read_with(cx, |v, _| (v.offers_reload(), v.changed_sent));
    assert_eq!(viewer, (false, false), "no window, no checks");
}
