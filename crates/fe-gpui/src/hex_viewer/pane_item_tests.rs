//! A [`HexViewer`] as a pane item, through kiln-shell's item handle: never
//! dirty, its status items, a split, a saved layout, and its file changing
//! on disk under it.

use std::{cell::RefCell, fs::OpenOptions, rc::Rc, sync::Arc};

use fe_command::Value;
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::{CloseAnswer, ItemHandle, PaneItem as _};

use super::tests::{counting, draw, file_of, viewer};
use super::*;
use crate::pane_item::status;

/// `state` restored, once the task is done.
fn restore(cx: &mut VisualTestContext, state: Value) -> Option<Entity<HexViewer>> {
    let task = cx.update(|window, cx| HexViewer::restore(state, window, cx));
    let result = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let result = result.clone();
        cx.spawn(async move |_| *result.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    result.take().expect("done")
}

fn status_texts(item: &dyn ItemHandle, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    cx.update(|_, cx| item.status_items(cx))
        .iter()
        .map(|item| (item.id.to_string(), item.text.to_string()))
        .collect()
}

#[gpui::test]
fn never_dirty_and_shows_read_only_and_the_offset(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(0x2000));
    let (viewer, cx, events) = viewer(cx, &path);
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    assert_eq!(item.kind(), "fastedit.hex_viewer");
    cx.update(|window, cx| {
        assert_eq!(item.tab_title(cx), "data.bin");
        assert_eq!(item.window_title(cx), "data.bin — Tenmoku");
        assert_eq!(item.path(cx), Some(path.clone()));
        assert!(!item.is_dirty(cx));
        assert!(item.save(window, cx).is_none(), "nothing to save");
        assert!(matches!(item.may_close(window, cx), CloseAnswer::Close));
    });
    let read_only = ("read_only".to_string(), "Read-only".to_string());
    let cursor = |text: &str| (status::CURSOR.to_string(), text.to_string());
    assert_eq!(
        status_texts(&*item, cx),
        [read_only.clone(), cursor("Offset 0x0")]
    );

    viewer.update(cx, |viewer, cx| {
        viewer.select(0x1A2F, 0x1A2F);
        cx.notify();
    });
    assert_eq!(
        status_texts(&*item, cx),
        [read_only.clone(), cursor("Offset 0x1A2F")]
    );
    cx.simulate_keystrokes("shift-right shift-right");
    assert_eq!(
        status_texts(&*item, cx),
        [read_only, cursor("Offset 0x1A2F–0x1A31, 3 bytes")]
    );

    // Typing changes nothing, so it's still never dirty.
    cx.simulate_input("x");
    cx.update(|window, cx| {
        assert!(!item.is_dirty(cx));
        assert!(matches!(item.may_close(window, cx), CloseAnswer::Close));
    });
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn a_truncated_file_shows_changed_with_reload(cx: &mut TestAppContext) {
    // Four chunks, so the end isn't in the cache the first screen filled.
    let len = 4 * fe_fileview::CHUNK_BYTES as usize;
    let (_dir, path) = file_of(&counting(len));
    let (viewer, cx, events) = viewer(cx, &path);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(100)
        .unwrap();

    // Going to the end reads past the cut: no panic, the banner offers a
    // reload.
    cx.simulate_keystrokes(&format!("{}-down", super::tests::secondary()));
    draw(cx);
    cx.run_until_parked();
    let (banner, offers) = viewer.read_with(cx, |v, _| {
        (v.banner().map(|b| b.to_string()), v.offers_reload())
    });
    assert_eq!(banner.as_deref(), Some("data.bin changed on disk."));
    assert!(offers);
    assert_eq!(*events.borrow(), [HexViewerEvent::Changed]);
    assert!(cx.debug_bounds(verbatim!("hex-viewer-reload")).is_some());

    // Reload: the file as it is now, the selection clipped to it.
    let reload = cx
        .debug_bounds(verbatim!("hex-viewer-reload"))
        .unwrap()
        .center();
    cx.simulate_click(reload, gpui::Modifiers::none());
    cx.run_until_parked();
    draw(cx);
    viewer.read_with(cx, |v, _| {
        assert_eq!(v.view().len(), 100);
        assert!(!v.offers_reload());
        assert_eq!(v.selected(), 99..100);
        assert_eq!(v.painted.last().map(|row| row.row), Some(6));
    });
    assert_eq!(
        *events.borrow(),
        [HexViewerEvent::Changed, HexViewerEvent::Reloaded]
    );

    // A change the screen never reads is found by the check of the file.
    std::fs::write(&path, b"rewritten").unwrap();
    cx.executor().advance_clock(CHECK_INTERVAL * 2);
    cx.run_until_parked();
    assert!(viewer.read_with(cx, |v, _| v.offers_reload()));
    assert_eq!(events.borrow().last(), Some(&HexViewerEvent::Changed));
}

#[gpui::test]
fn restores_path_and_selection_clipped(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(4096));
    let (viewer, cx, _) = viewer(cx, &path);
    viewer.update(cx, |viewer, cx| {
        viewer.select(3000, 3010);
        cx.notify();
    });
    draw(cx);
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    let state = cx.update(|_, cx| item.serialize(cx)).expect("saved");
    assert_eq!(state.get("path").and_then(Value::as_str), path.to_str());
    assert_eq!(state.get("anchor").and_then(Value::as_u64), Some(3000));
    assert_eq!(state.get("head").and_then(Value::as_u64), Some(3010));
    let top = viewer.read_with(cx, |v, _| v.top_row());
    assert!(top > 0);
    assert_eq!(state.get("top_row").and_then(Value::as_u64), Some(top));

    let restored = restore(cx, state.clone()).expect("restored");
    restored.read_with(cx, |v, _| {
        assert_eq!(v.selection_ends(), (3000, 3010));
        assert_eq!(v.top_row(), top);
    });

    // The file shrank since: clipped to it.
    std::fs::write(&path, counting(3005)).unwrap();
    let restored = restore(cx, state).expect("restored");
    restored.read_with(cx, |v, _| {
        assert_eq!(v.selection_ends(), (3000, 3004));
        assert_eq!(v.selected(), 3000..3005);
    });
}

#[gpui::test]
fn a_missing_file_is_not_restored(cx: &mut TestAppContext) {
    let (dir, path) = file_of(&counting(64));
    let (viewer, cx, _) = viewer(cx, &path);
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    let state = cx.update(|_, cx| item.serialize(cx)).expect("saved");
    std::fs::remove_file(&path).unwrap();
    assert!(restore(cx, state).is_none());
    // A directory, or no path, neither.
    let state = Value::from_iter([("path", Value::from(dir.path().to_str().unwrap()))]);
    assert!(restore(cx, state).is_none());
    assert!(restore(cx, Value::from_iter([("head", Value::from(3u64))])).is_none());
}

#[gpui::test]
fn split_shares_the_view(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(1024));
    let (_other_dir, other_path) = file_of(&counting(10));
    let (viewer, cx, _) = viewer(cx, &path);
    viewer.update(cx, |viewer, cx| {
        viewer.select(40, 70);
        viewer.column = Column::Ascii;
        cx.notify();
    });
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    let split = cx
        .update(|window, cx| item.split(window, cx))
        .expect("splits");
    let other: Box<dyn ItemHandle> = Box::new(cx.update(|_, cx| {
        let view = Arc::new(FileView::open(&other_path).unwrap());
        let theme = EditorTheme::default_dark(cx);
        cx.new(|cx| HexViewer::new(view, theme, cx))
    }));
    cx.update(|_, cx| {
        assert!(item.shares_document(&*split, cx));
        assert!(split.shares_document(&*item, cx));
        assert!(!item.shares_document(&*other, cx), "another file");
        assert_eq!(split.path(cx), Some(path.clone()));
    });
    let split = split.downcast::<HexViewer>().expect("a hex viewer");
    split.read_with(cx, |split, cx| {
        let original = viewer.read(cx);
        assert!(Arc::ptr_eq(&split.view, &original.view));
        assert_eq!(split.selection_ends(), (40, 70));
        assert_eq!(split.column(), Column::Ascii);
    });
}
