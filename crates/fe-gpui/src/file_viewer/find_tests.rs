//! Find in a [`FileViewer`]: stepping, stopping and the rows found so far.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

use fe_fileview::{FindOutcome, FindQuery, IndexState, LineIndex};
use futures::channel::oneshot;
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
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

fn query(needle: &str) -> Option<FindQuery> {
    Some(FindQuery {
        needle: needle.into(),
        case_sensitive: true,
    })
}

fn selection(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> Range<usize> {
    viewer.read_with(cx, |viewer, _| viewer.model().selection())
}

#[gpui::test]
fn next_and_previous(cx: &mut TestAppContext) {
    let text = "one\ntwo needle\nthree\nfour needle\nfive";
    let (_dir, view, _) = write_file(text, text.lines().map(str::to_owned).collect());
    let index = complete_index(&view);
    let (viewer, cx, events) = viewer(cx, view, index);
    let first = text.find("needle").unwrap();
    let second = text.rfind("needle").unwrap();
    let step = |cx: &mut VisualTestContext, next: bool| {
        viewer.update(cx, |viewer, cx| {
            if next {
                viewer.find_next(cx)
            } else {
                viewer.find_prev(cx)
            }
        });
        cx.run_until_parked();
    };

    viewer.update(cx, |viewer, cx| viewer.set_find(query("needle"), cx));
    step(cx, true);
    assert_eq!(selection(&viewer, cx), first..first + 6);
    step(cx, true);
    assert_eq!(selection(&viewer, cx), second..second + 6);
    step(cx, false);
    assert_eq!(selection(&viewer, cx), first..first + 6);
    step(cx, true);
    step(cx, true);
    assert_eq!(selection(&viewer, cx), first..first + 6);
    assert_eq!(
        *events.borrow(),
        [
            FileViewerEvent::Found { wrapped: false },
            FileViewerEvent::Found { wrapped: false },
            FileViewerEvent::Found { wrapped: false },
            FileViewerEvent::Found { wrapped: false },
            FileViewerEvent::Found { wrapped: true },
        ]
    );

    events.borrow_mut().clear();
    viewer.update(cx, |viewer, cx| viewer.set_find(query("absent"), cx));
    step(cx, true);
    assert_eq!(selection(&viewer, cx), first..first + 6);
    assert_eq!(*events.borrow(), [FileViewerEvent::NotFound]);
}

#[gpui::test]
fn a_match_off_screen_is_scrolled_to(cx: &mut TestAppContext) {
    let (_dir, view, _) = generated(8 * 1024 * 1024);
    let index = complete_index(&view);
    let (viewer, cx, _events) = viewer(cx, view, index);
    let top_row = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |v, _| v.painted_rows.first().map(|row| row.0))
    };
    let shows = |cx: &mut VisualTestContext, row: u32| {
        painted(&viewer, cx)
            .iter()
            .any(|(painted, ..)| *painted == row)
    };
    assert_eq!(top_row(cx), Some(0));

    viewer.update(cx, |viewer, cx| {
        viewer.set_find(query("row 00100000"), cx);
        viewer.find_next(cx);
    });
    cx.run_until_parked();
    let at = 100_000 * 64;
    assert_eq!(selection(&viewer, cx), at..at + 12);
    assert!(shows(cx, 100_000), "{:?}", top_row(cx));
    assert!(viewer.read_with(cx, |v, _| v.scroll.y > px(0.)));

    viewer.update(cx, |viewer, cx| {
        viewer.set_find(query("row 00000100"), cx);
        viewer.find_prev(cx);
    });
    cx.run_until_parked();
    assert_eq!(selection(&viewer, cx), 100 * 64..100 * 64 + 12);
    assert!(shows(cx, 100), "{:?}", top_row(cx));
    assert!(!shows(cx, 100_000));
}

#[gpui::test]
fn a_newer_search_stops_the_last(cx: &mut TestAppContext) {
    let (_dir, view, _) = generated(8 * 1024 * 1024);
    let index = complete_index(&view);
    let (viewer, cx, events) = viewer(cx, view, index);
    let hold = viewer.read_with(cx, |v, _| v.find.hold.clone());
    let finding = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.is_finding());
    let flag = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |v, _| v.stop_flag().expect("a search runs"))
    };
    let stopped = |flag: &Arc<AtomicBool>| flag.load(Ordering::Relaxed);
    let ended = || hold.ended.lock().unwrap().clone();
    // Queues a hold for the next scan, which the sender releases.
    let hold_next = || {
        let (release, held) = oneshot::channel();
        hold.releases.lock().unwrap().push_back(held);
        release
    };
    // Runs the executor until `n` scans have begun and every task waits.
    let until_begun = |cx: &mut VisualTestContext, n: usize| {
        cx.run_until_parked();
        assert_eq!(hold.begun.load(Ordering::SeqCst), n);
    };
    let query_text = "row 00001000";
    viewer.update(cx, |viewer, cx| viewer.set_find(query(query_text), cx));
    assert!(!finding(cx));

    // Replaced: forward finds the match without wrapping, backward from the
    // start only by wrapping, so the event says which landed.
    let release = hold_next();
    viewer.update(cx, |viewer, cx| viewer.find_next(cx));
    let first = flag(cx);
    until_begun(cx, 1);
    assert!(finding(cx) && !stopped(&first) && ended().is_empty());
    viewer.update(cx, |viewer, cx| viewer.find_prev(cx));
    assert!(stopped(&first));
    until_begun(cx, 2);
    assert_eq!(
        ended(),
        [Some(FindOutcome::Found {
            range: 1000 * 64..1000 * 64 + 12,
            wrapped: true
        })]
    );
    assert_eq!(*events.borrow(), [FileViewerEvent::Found { wrapped: true }]);
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(ended().last(), Some(&Some(FindOutcome::Stopped)));
    assert_eq!(ended().len(), 2);
    assert_eq!(*events.borrow(), [FileViewerEvent::Found { wrapped: true }]);
    assert!(!finding(cx));

    // Cleared.
    events.borrow_mut().clear();
    let release = hold_next();
    viewer.update(cx, |viewer, cx| viewer.find_next(cx));
    let second = flag(cx);
    until_begun(cx, 3);
    assert!(finding(cx) && !stopped(&second) && ended().len() == 2);
    viewer.update(cx, |viewer, cx| viewer.set_find(None, cx));
    assert!(stopped(&second) && !finding(cx));
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(ended().len(), 3);
    assert_eq!(ended().last(), Some(&Some(FindOutcome::Stopped)));
    assert_eq!(*events.borrow(), []);

    // Reloaded.
    viewer.update(cx, |viewer, cx| {
        viewer.set_find(query(query_text), cx);
    });
    let release = hold_next();
    viewer.update(cx, |viewer, cx| viewer.find_next(cx));
    let third = flag(cx);
    until_begun(cx, 4);
    assert!(finding(cx) && !stopped(&third) && ended().len() == 3);
    viewer.update(cx, |viewer, cx| viewer.reload(cx));
    assert!(stopped(&third) && !finding(cx));
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(ended().len(), 4);
    assert_eq!(ended().last(), Some(&Some(FindOutcome::Stopped)));
    assert_eq!(*events.borrow(), [FileViewerEvent::Reloaded]);
}

#[gpui::test]
fn only_indexed_rows(cx: &mut TestAppContext) {
    let (_dir, view, _) = generated(18_000 * 64);
    let (index, mut step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    step.next();
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    let indexed = viewer.read_with(cx, |v, _| v.model().text().len());
    assert!(indexed < 18_000 * 64);

    viewer.update(cx, |viewer, cx| {
        viewer.set_find(query("row 00017000"), cx);
        viewer.find_next(cx);
    });
    cx.run_until_parked();
    assert_eq!(selection(&viewer, cx), 0..0);
    assert_eq!(*events.borrow(), [FileViewerEvent::NotFound]);

    step.for_each(drop);
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    viewer.update(cx, |viewer, cx| viewer.find_next(cx));
    cx.run_until_parked();
    let at = 17_000 * 64;
    assert_eq!(selection(&viewer, cx), at..at + 12);
}
