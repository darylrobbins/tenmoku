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
        assert_eq!(item.window_title(cx), "main.rs — fastedit");
        assert!(!item.is_dirty(cx));
        assert_eq!(item.path(cx), Some(path.clone()));
        assert_eq!(item.tab_icon(cx), None, "the tab draws none");
    });

    view.update(cx, |view, cx| {
        view.model_mut().insert("// ");
        cx.notify();
    });
    cx.update(|_, cx| {
        assert!(item.is_dirty(cx));
        assert_eq!(item.window_title(cx), "main.rs ● — fastedit");
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
