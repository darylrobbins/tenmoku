//! Saving trims trailing whitespace and adds a final newline where asked,
//! as one edit that undoes.

use std::fs;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::SaveFixes;
use crate::{EditorTheme, EditorView};

const BOTH: SaveFixes = SaveFixes {
    trim_trailing_whitespace: true,
    insert_final_newline: true,
};

fn opened<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    fixes: SaveFixes,
) -> (
    tempfile::TempDir,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    cx.update(crate::init);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, text).unwrap();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = fe_editor::EditorModel::open(&path).unwrap();
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    view.update(cx, |view, _| view.set_save_fixes(fixes));
    (dir, view, cx)
}

fn save(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let saved = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    cx.run_until_parked();
    drop(saved);
}

#[gpui::test]
fn trim_and_final_newline(cx: &mut TestAppContext) {
    let (dir, view, cx) = opened(cx, "x\n", BOTH);
    let path = dir.path().join("a.txt");
    let text = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model().text().to_string())
    };

    // Typed over the file, saved trimmed with a final newline, and one
    // undo takes both back.
    view.update(cx, |view, _| {
        view.model_mut().select_all();
        view.model_mut().insert("a  \nb\t");
    });
    save(&view, cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), "a\nb\n");
    assert_eq!(text(&view, cx), "a\nb\n");
    view.update(cx, |view, _| {
        view.model_mut().undo().unwrap();
    });
    assert_eq!(text(&view, cx), "a  \nb\t");

    // Clean text makes no transaction: the one undo is the typing's.
    view.update(cx, |view, _| {
        view.model_mut().select_all();
        view.model_mut().insert("clean\n");
    });
    save(&view, cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), "clean\n");
    view.update(cx, |view, _| {
        view.model_mut().undo().unwrap();
    });
    assert_eq!(text(&view, cx), "a  \nb\t");

    // Whitespace-only text trims to nothing and gets no newline.
    view.update(cx, |view, _| {
        view.model_mut().select_all();
        view.model_mut().insert("  ");
    });
    save(&view, cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), "");

    // Both off: saved as typed.
    view.update(cx, |view, _| {
        view.set_save_fixes(SaveFixes::default());
        view.model_mut().select_all();
        view.model_mut().insert("a  \nb\t");
    });
    save(&view, cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), "a  \nb\t");
}

#[gpui::test]
fn crlf_buffer(cx: &mut TestAppContext) {
    let (dir, view, cx) = opened(cx, "x\r\ny\r\n", BOTH);
    view.update(cx, |view, _| {
        view.model_mut().select_all();
        view.model_mut().insert("a  \nb\t");
    });
    save(&view, cx);
    assert_eq!(fs::read(dir.path().join("a.txt")).unwrap(), b"a\r\nb\r\n");
}

#[gpui::test]
fn edited_through_a_split(cx: &mut TestAppContext) {
    let (dir, view, cx) = opened(cx, "a \n", BOTH);
    let split = view.update(cx, |view, _| view.model().split());
    let (other, cx) =
        cx.add_window_view(move |_, cx| EditorView::new(split, EditorTheme::default_dark(cx), cx));
    // Edited through the other view, then saved before anything redraws.
    other.update(cx, |view, _| {
        view.model_mut().select_all();
        view.model_mut().insert("xyz");
    });
    save(&view, cx);
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "xyz\n"
    );
}

#[gpui::test]
fn apart_from_an_open_transaction(cx: &mut TestAppContext) {
    let (dir, view, cx) = opened(cx, "x\n", BOTH);
    view.update(cx, |view, _| {
        view.model_mut()
            .buffer_mut()
            .start_transaction(fe_buffer::Origin::User);
        view.model_mut().select_all();
        view.model_mut().insert("a  ");
    });
    save(&view, cx);
    assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "a\n");
    view.update(cx, |view, _| {
        view.model_mut().undo().unwrap();
        assert_eq!(view.model().text().to_string(), "a  ");
    });
}
