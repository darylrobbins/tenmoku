//! A [`FileViewer`] over generated files, in a headless window, with keys
//! through the real keymap.

use std::{cell::RefCell, fs::OpenOptions, rc::Rc, sync::Arc};

use fe_fileview::{FileView, IndexState, LineIndex};
use gpui::{
    AppContext as _, ClipboardItem, Entity, Modifiers, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, VisualTestContext, point, px,
};
use tempfile::TempDir;

use super::*;
use crate::EditorTheme;

/// A file of exactly `len` bytes, a multiple of 64, and its rows: each
/// row 63 bytes and a `\n`, but the last, which takes the last row's `\n`
/// as one more char.
fn generated(len: usize) -> (TempDir, Arc<FileView>, Vec<String>) {
    assert_eq!(len % 64, 0);
    let rows = len / 64;
    let mut lines: Vec<String> = (0..rows)
        .map(|row| {
            let mut line = format!("row {row:08} {}", "abcdefghijklmnopqrstuvwxyz".repeat(3));
            line.truncate(63);
            line
        })
        .collect();
    lines.last_mut().expect("rows").push('$');
    let text = lines.join("\n");
    assert_eq!(text.len(), len);
    write_file(&text, lines)
}

fn write_file(text: &str, lines: Vec<String>) -> (TempDir, Arc<FileView>, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.log");
    std::fs::write(&path, text).unwrap();
    let view = FileView::open(&path).unwrap();
    assert_eq!(view.len(), text.len() as u64);
    (dir, Arc::new(view), lines)
}

/// A whole index of `view`, built a chunk at a time on this thread.
fn complete_index(view: &Arc<FileView>) -> Arc<LineIndex> {
    let (index, step) = LineIndex::build_stepped(view.clone());
    step.for_each(drop);
    assert_eq!(index.progress().state, IndexState::Complete);
    Arc::new(index)
}

type Events = Rc<RefCell<Vec<FileViewerEvent>>>;

/// The window's root, which makes the viewer and subscribes to it at once,
/// as a workbench tab would.
struct Host {
    viewer: Entity<FileViewer>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.viewer.clone()
    }
}

/// A focused viewer in a headless window, and the events it sends.
fn viewer(
    cx: &mut TestAppContext,
    view: Arc<FileView>,
    index: Arc<LineIndex>,
) -> (Entity<FileViewer>, &mut VisualTestContext, Events) {
    cx.update(crate::init);
    let events = Events::default();
    let heard = events.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let theme = EditorTheme::default_dark(cx);
        let banner = "Read only: too large to edit".into();
        let viewer = cx.new(|cx| FileViewer::new(view, index, banner, theme, cx));
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

fn painted(
    viewer: &Entity<FileViewer>,
    cx: &mut VisualTestContext,
) -> Vec<(u32, String, Point<Pixels>)> {
    viewer.read_with(cx, |viewer, _| viewer.painted_rows.clone())
}

/// The top of the text area, below the banner.
fn text_top(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> Pixels {
    viewer.read_with(cx, |viewer, _| {
        viewer.layout.as_ref().expect("drawn").origin.y
    })
}

/// The centre of a caret at `offset`, which must be on screen.
fn position_of(
    viewer: &Entity<FileViewer>,
    cx: &mut VisualTestContext,
    offset: usize,
) -> Point<Pixels> {
    viewer.update(cx, |viewer, _| {
        let layout = viewer.layout.as_mut().expect("drawn");
        layout.caret_bounds(offset).expect("visible").center()
    })
}

#[gpui::test]
fn paints_the_first_rows_and_the_end(cx: &mut TestAppContext) {
    let (_dir, view, lines) = generated(8 * 1024 * 1024);
    let index = complete_index(&view);
    let (viewer, cx, events) = viewer(cx, view, index);

    let top = text_top(&viewer, cx);
    let line_height = viewer.read_with(cx, |viewer, _| {
        px(viewer.layout.as_ref().unwrap().frame.line_height)
    });
    assert!(top >= line_height, "the banner is above the text: {top:?}");
    let rows = painted(&viewer, cx);
    assert!(rows.len() > 10, "{} rows", rows.len());
    for (ix, (row, text, origin)) in rows.iter().enumerate() {
        assert_eq!(*row as usize, ix);
        assert_eq!(text, &lines[ix]);
        assert!(origin.y >= top, "row {row} at {origin:?}, under {top:?}");
    }

    let over_text = point(px(200.), top + px(100.));
    cx.simulate_event(ScrollWheelEvent {
        position: over_text,
        delta: ScrollDelta::Lines(point(0., -(lines.len() as f32 * 2.))),
        ..Default::default()
    });
    cx.run_until_parked();
    let rows = painted(&viewer, cx);
    let (last, ..) = rows.last().expect("rows");
    assert_eq!(*last as usize, lines.len() - 1);
    for (row, text, origin) in &rows {
        assert_eq!(text, &lines[*row as usize]);
        assert!(origin.y >= top - line_height, "row {row} at {origin:?}");
    }
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn selects_and_copies(cx: &mut TestAppContext) {
    let text = "zero one two\nfirst row\nsecond row here\nthird";
    let lines = text.lines().map(str::to_owned).collect();
    let (_dir, view, _) = write_file(text, lines);
    let index = complete_index(&view);
    let (viewer, cx, events) = viewer(cx, view, index);
    let selection = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().selection());

    let second = text.find("second").unwrap();
    let from = position_of(&viewer, cx, 5);
    cx.simulate_click(from, Modifiers::none());
    assert_eq!(selection(cx), 5..5);
    let to = position_of(&viewer, cx, second + 3);
    cx.simulate_click(to, Modifiers::shift());
    assert_eq!(selection(cx), 5..second + 3);

    cx.simulate_keystrokes("cmd-c");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(&text[5..second + 3]));
    assert_eq!(*events.borrow(), []);

    let unchanged = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |viewer, _| {
            let model = viewer.model();
            (
                model.text().slice_to_string(0..model.text().len()),
                model.selection(),
            )
        })
    };
    cx.simulate_keystrokes("x");
    assert_eq!(*events.borrow(), [FileViewerEvent::ReadOnly]);
    assert_eq!(unchanged(cx), (text.to_owned(), 5..second + 3));

    cx.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(
        *events.borrow(),
        [FileViewerEvent::ReadOnly, FileViewerEvent::ReadOnly]
    );
    assert_eq!(unchanged(cx), (text.to_owned(), 5..second + 3));
}

#[gpui::test]
fn rows_grow_while_indexing(cx: &mut TestAppContext) {
    // About 4.4 chunks.
    let (_dir, view, lines) = generated(18_000 * 64);
    let (index, step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    let rows =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().display().row_count());
    let polling = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.is_polling());

    let mut before = rows(cx);
    assert_eq!(before, 1);
    assert!(polling(cx));
    let mut steps = 0;
    for progress in step {
        steps += 1;
        cx.executor().advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
        let now = rows(cx);
        assert!(now > before, "step {steps}: {now} rows after {before}");
        before = now;
        assert_eq!(polling(cx), progress.state == IndexState::Building);
    }
    assert!(steps >= 4, "{steps} steps");
    assert_eq!(before as usize, lines.len());
    assert!(!polling(cx));
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn completing_before_the_first_poll_shows_every_row(cx: &mut TestAppContext) {
    let (_dir, view, lines) = generated(18_000 * 64);
    let (index, step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    assert!(viewer.read_with(cx, |v, _| v.is_polling()));
    step.for_each(drop);

    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.model().display().row_count() as usize, lines.len());
        assert!(!viewer.is_polling());
    });
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn an_index_that_failed_before_the_view_is_reported(cx: &mut TestAppContext) {
    let (dir, view, _) = generated(18_000 * 64);
    let (index, step) = LineIndex::build_stepped(view.clone());
    // Shorter than recorded: the scan reads short and stops.
    let file = OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"));
    file.unwrap().set_len(1000).unwrap();
    step.for_each(drop);
    assert_eq!(index.progress().state, IndexState::Changed);

    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
    assert!(!viewer.read_with(cx, |v, _| v.is_polling()));
}

#[gpui::test]
fn a_copy_that_reads_a_truncated_file_reports_it_once(cx: &mut TestAppContext) {
    // Bigger than the copy's chunk reads around the view's two ends.
    let (dir, view, _) = generated(18_000 * 64);
    let index = complete_index(&view);
    let (_viewer, cx, events) = viewer(cx, view, index);
    cx.simulate_keystrokes("cmd-a");
    assert_eq!(*events.borrow(), []);

    let file = OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"));
    file.unwrap().set_len(1000).unwrap();
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}

#[gpui::test]
fn a_refresh_that_finds_fewer_rows_keeps_the_last_snapshot(cx: &mut TestAppContext) {
    let (_dir, view, _) = generated(18_000 * 64);
    let (index, mut step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view.clone(), Arc::new(index));
    step.next();
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    let rows =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().display().row_count());
    let shown = rows(cx);
    assert!(shown > 1);

    // The snapshot a racing read would make: one row.
    let (fresh, _unscanned) = LineIndex::build_stepped(view);
    viewer.update(cx, |v, cx| {
        v.index = Arc::new(fresh);
        v.refresh(cx);
    });
    assert_eq!(rows(cx), shown);
    assert!(!viewer.read_with(cx, |v, _| v.is_polling()));
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}

#[gpui::test]
fn an_index_that_changes_while_polling_keeps_its_rows(cx: &mut TestAppContext) {
    let (dir, view, _) = generated(18_000 * 64);
    let (index, mut step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    step.next();
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    let rows =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().display().row_count());
    let shown = rows(cx);
    assert!(shown > 1);
    let generation = viewer.read_with(cx, |v, _| v.generation);

    let file = OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"));
    file.unwrap().set_len(1000).unwrap();
    step.for_each(drop);
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(rows(cx), shown);
    // The poll saw the failure and installed no snapshot.
    assert_eq!(viewer.read_with(cx, |v, _| v.generation), generation);
    assert!(!viewer.read_with(cx, |v, _| v.is_polling()));
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}
