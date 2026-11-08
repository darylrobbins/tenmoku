//! An editor as a pane item: its tab's and the window's titles, dirty
//! state, file, saving, splitting over its buffer, and saved state,
//! through kiln-shell's item handle.

use std::{cell::RefCell, fs, path::PathBuf, rc::Rc};

use fe_buffer::Buffer;
use fe_command::Value;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::ItemHandle;

use super::status;
use crate::{EditorTheme, EditorView};

/// An editor over `text`, at `path`, in a headless window.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    path: Option<PathBuf>,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let text = text.to_owned();
    cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), path);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    })
}

#[gpui::test]
fn titles_and_dirty_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    let (view, cx) = editor(cx, "fn main() {}\n", Some(path.clone()));
    let item: Box<dyn ItemHandle> = Box::new(view.clone());
    assert_eq!(item.kind(), "fastedit.editor");
    cx.update(|_, cx| {
        assert_eq!(item.tab_title(cx), "main.rs");
        assert_eq!(item.window_title(cx), "main.rs — Tenmoku");
        assert!(!item.is_dirty(cx));
        assert_eq!(item.path(cx), Some(path.clone()));
        assert_eq!(item.tab_icon(cx), Some(kiln_shell::Icon::FileCode));
    });

    view.update(cx, |view, cx| {
        view.model_mut().insert("// ");
        cx.notify();
    });
    cx.update(|_, cx| {
        assert!(item.is_dirty(cx));
        assert_eq!(item.window_title(cx), "main.rs ● — Tenmoku");
    });

    // Saving writes the file and clears the dot.
    let saved = cx
        .update(|window, cx| item.save(window, cx))
        .expect("saves");
    let result = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let result = result.clone();
        cx.spawn(async move |_| *result.borrow_mut() = Some(saved.await))
            .detach();
    });
    cx.run_until_parked();
    result.take().expect("done").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "// fn main() {}\n");
    assert!(cx.update(|_, cx| !item.is_dirty(cx)));
}

#[gpui::test]
fn an_untitled_buffer(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "", None);
    let item: Box<dyn ItemHandle> = Box::new(view);
    cx.update(|_, cx| {
        assert_eq!(item.tab_title(cx), "untitled");
        assert_eq!(item.tab_icon(cx), Some(kiln_shell::Icon::File));
        assert_eq!(item.path(cx), None);
        assert_eq!(item.serialize(cx), None, "left out of a saved layout");
    });
}

#[gpui::test]
fn a_split_is_another_view_of_the_buffer(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one\n", Some("/tmp/a.txt".into()));
    let item: Box<dyn ItemHandle> = Box::new(view.clone());
    let split = cx
        .update(|window, cx| item.split(window, cx))
        .expect("splits");
    let other: Box<dyn ItemHandle> = Box::new(cx.update(|_, cx| {
        gpui::AppContext::new(cx, |cx| {
            let model = EditorModel::new(Buffer::new("one\n"), Some("/tmp/a.txt".into()));
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        })
    }));
    cx.update(|_, cx| {
        assert!(item.shares_document(&*split, cx));
        assert!(split.shares_document(&*item, cx));
        assert!(
            !item.shares_document(&*other, cx),
            "the same file, read twice"
        );
    });
    let split = split.downcast::<EditorView>().expect("an editor");
    view.update(cx, |view, cx| {
        view.model_mut().insert("zero ");
        cx.notify();
    });
    let text = split.update(cx, |split, _| {
        split.model_mut().sync_display();
        split.model().text().to_string()
    });
    assert_eq!(text, "zero one\n");
}

#[gpui::test]
fn saved_state_is_the_path_and_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "abc\ndef\n", Some("/tmp/b.txt".into()));
    view.update(cx, |view, _| view.model_mut().insert("xy"));
    let item: Box<dyn ItemHandle> = Box::new(view);
    let state = cx.update(|_, cx| item.serialize(cx));
    let expected = Value::from_iter([
        ("path", Value::from("/tmp/b.txt")),
        ("caret", Value::from(2)),
    ]);
    assert_eq!(state, Some(expected));
}

#[gpui::test]
fn status_items_are_the_editors(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "abc\ndef\n", None);
    view.update(cx, |view, _| view.model_mut().insert("é"));
    let item: Box<dyn ItemHandle> = Box::new(view);
    let items = cx.update(|_, cx| item.status_items(cx));
    let drawn: Vec<_> = items
        .iter()
        .map(|item| (item.side, item.id.as_ref(), item.text.as_ref()))
        .collect();
    let right = kiln_shell::StatusSide::Right;
    assert_eq!(
        drawn,
        [
            (right, status::CURSOR, "Ln 1, Col 2"),
            (right, status::INDENT, "Spaces: 4"),
            (right, status::ENCODING, "UTF-8"),
            (right, status::LINE_ENDING, "LF"),
            (right, status::LANGUAGE, "Plain Text"),
        ]
    );
    let language = items.last().unwrap().command.as_ref().unwrap();
    assert_eq!(language.command.as_str(), "language.select");
    assert_eq!(status::language_label("cpp"), "C++");
    assert_eq!(status::language_label("zig"), "zig");
}

#[test]
fn newer_languages_have_labels() {
    for (id, label) in [
        ("gitcommit", "Git Commit Message"),
        ("gitrebase", "Git Rebase Todo"),
        ("gitconfig", "Git Config"),
        ("gitattributes", "Git Attributes"),
        ("gitignore", "Ignore"),
        ("ini", "INI"),
        ("dotenv", "Dotenv"),
        ("just", "Just"),
        ("diff", "Diff"),
        ("json5", "JSON5"),
        ("csv", "CSV"),
        ("tsv", "TSV"),
    ] {
        assert_eq!(status::language_label(id), label, "{id}");
    }
}

#[gpui::test]
fn a_restored_editor_refuses_a_file_changed_since(cx: &mut TestAppContext) {
    use kiln_shell::PaneItem as _;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restored.txt");
    fs::write(&path, "read\n").unwrap();
    let (_, cx) = editor(cx, "", None);
    let state = Value::from_iter([
        ("path", Value::from(path.to_str().unwrap())),
        ("caret", Value::from(0)),
    ]);
    let task = cx.update(|window, cx| EditorView::restore(state, window, cx));
    let restored = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let restored = restored.clone();
        cx.spawn(async move |_| *restored.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    let view = restored.take().flatten().expect("restored");

    fs::write(&path, "theirs\n").unwrap();
    let saved = view.update(cx, |view, _| {
        view.model_mut().insert("mine ");
        view.model_mut().save()
    });
    assert!(saved.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
}

use std::{sync::Arc, thread, time::Duration};

use fe_buffer::{Origin, UndoRecord, UndoStore};

use crate::pane_item::{UndoPolicy, UndoWrites};

fn undo_record(text: &str) -> UndoRecord {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, text)], Origin::User);
    buffer.export_undo().unwrap()
}

fn undo_policy(window: gpui::WindowId, on: bool) -> UndoPolicy {
    UndoPolicy {
        windows: [(window, on)].into_iter().collect(),
        fallback: true,
    }
}

/// Starts a writer for `path` and returns once it has copied its
/// capture and is blocked on the policy lock, which the caller holds.
fn start_writer(
    writes: &Arc<UndoWrites>,
    store: &Arc<UndoStore>,
    path: &std::path::Path,
) -> thread::JoinHandle<()> {
    let writer = {
        let (writes, store, path) = (writes.clone(), store.clone(), path.to_path_buf());
        thread::spawn(move || writes.flush(&store, &path))
    };
    while writes.writing.try_lock().is_ok() && !writer.is_finished() {
        thread::yield_now();
    }
    thread::sleep(Duration::from_millis(200));
    writer
}

/// What `publish(off)` then `publish(on)` do, under the policy lock.
fn off_then_on(writes: &UndoWrites, guard: &mut Option<UndoPolicy>, window: gpui::WindowId) {
    let off = undo_policy(window, false);
    *guard = Some(off.clone());
    writes
        .pending
        .lock()
        .unwrap()
        .retain(|_, capture| off.allows(capture.window));
    *guard = Some(undo_policy(window, true));
}

fn setup() -> (
    tempfile::TempDir,
    Arc<UndoStore>,
    std::path::PathBuf,
    Arc<UndoWrites>,
    UndoRecord,
    gpui::WindowId,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(UndoStore::new(dir.path().join("undo")));
    let path = dir.path().join("a.txt");
    let old = undo_record("old");
    let _ = store.write(&path, &old);
    (
        dir,
        store,
        path,
        Arc::new(UndoWrites::default()),
        old,
        gpui::WindowId::from(1u64),
    )
}

#[test]
fn a_writer_holding_a_capture_dropped_by_off_then_on_writes_nothing() {
    let (_dir, store, path, writes, old, window) = setup();
    writes.capture(path.clone(), Some(undo_record("new")), Some(window));
    let mut in_flight = writes.policy.lock().unwrap();
    let writer = start_writer(&writes, &store, &path);
    off_then_on(&writes, &mut in_flight, window);
    drop(in_flight);
    writer.join().unwrap();
    assert_eq!(store.read(&path).unwrap(), Some(old));
    assert_eq!(writes.waiting(&path), None);
}

#[test]
fn a_writer_holding_a_removal_dropped_by_off_then_on_removes_nothing() {
    let (_dir, store, path, writes, old, window) = setup();
    writes.capture(path.clone(), None, Some(window));
    let mut in_flight = writes.policy.lock().unwrap();
    let writer = start_writer(&writes, &store, &path);
    off_then_on(&writes, &mut in_flight, window);
    drop(in_flight);
    writer.join().unwrap();
    assert_eq!(store.read(&path).unwrap(), Some(old));
    assert_eq!(writes.waiting(&path), None);
}

#[test]
fn a_newer_capture_taken_while_a_writer_waits_is_still_written() {
    let (_dir, store, path, writes, _old, window) = setup();
    writes.capture(path.clone(), Some(undo_record("older")), Some(window));
    let in_flight = writes.policy.lock().unwrap();
    let writer = start_writer(&writes, &store, &path);
    let newer = undo_record("newer");
    writes.capture(path.clone(), Some(newer.clone()), Some(window));
    drop(in_flight);
    writer.join().unwrap();
    writes.flush(&store, &path);
    assert_eq!(store.read(&path).unwrap(), Some(newer));
    assert_eq!(writes.waiting(&path), None);
}
