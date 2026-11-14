//! A file opens clean whatever its `.editorconfig` says about `charset` and
//! `end_of_line` (T378, Mac T1828); the first save after an edit converts
//! it, and saving an unedited file leaves its bytes alone.

use std::path::Path;

use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::tests::open_root;

/// `bytes` as `a.txt`-like `name` under `config`, opened in a tab.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &Path,
    config: &str,
    name: &str,
    bytes: &[u8],
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    std::fs::write(dir.join(".editorconfig"), config).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let (workbench, cx) = open_root(cx, dir);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
    let editor = workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().expect("a tab"));
    (editor, cx)
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

fn save(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let saved = cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
    cx.run_until_parked();
    drop(saved);
}

#[gpui::test]
fn a_bom_file_under_charset_utf8_opens_clean(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (editor, cx) = opened(
        cx,
        dir.path(),
        "root = true\n[*.cs]\ncharset = utf-8\n",
        "a.cs",
        b"\xEF\xBB\xBFclass A {}\n",
    );
    assert!(!dirty(&editor, cx));
    // Unedited, a save changes nothing.
    save(&editor, cx);
    assert_eq!(
        std::fs::read(dir.path().join("a.cs")).unwrap(),
        b"\xEF\xBB\xBFclass A {}\n"
    );

    cx.simulate_input("x");
    assert!(dirty(&editor, cx));
    save(&editor, cx);
    assert_eq!(
        std::fs::read(dir.path().join("a.cs")).unwrap(),
        b"xclass A {}\n"
    );
    assert!(!dirty(&editor, cx));
}

#[gpui::test]
fn an_lf_file_under_end_of_line_crlf_opens_clean(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (editor, cx) = opened(
        cx,
        dir.path(),
        "root = true\n[*]\nend_of_line = crlf\n",
        "a.txt",
        b"one\ntwo\n",
    );
    assert!(!dirty(&editor, cx));
    save(&editor, cx);
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        b"one\ntwo\n"
    );

    cx.simulate_input("x");
    assert!(dirty(&editor, cx));
    save(&editor, cx);
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        b"xone\r\ntwo\r\n"
    );
    assert!(!dirty(&editor, cx));
}

#[gpui::test]
fn an_explicit_change_still_marks_the_tab_modified(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (editor, cx) = opened(
        cx,
        dir.path(),
        "root = true\n[*]\nend_of_line = crlf\n",
        "a.txt",
        b"one\n",
    );
    assert!(!dirty(&editor, cx));
    editor.update(cx, |e, _| {
        e.model_mut()
            .buffer_mut()
            .set_line_ending(fe_buffer::LineEnding::Lf);
    });
    assert!(!dirty(&editor, cx));
    editor.update(cx, |e, _| {
        e.model_mut()
            .buffer_mut()
            .set_line_ending(fe_buffer::LineEnding::CrLf);
    });
    assert!(dirty(&editor, cx));
    save(&editor, cx);
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"one\r\n");
}

#[gpui::test]
fn a_missing_file_under_both_settings_saves_with_both(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\ncharset = utf-8-bom\nend_of_line = crlf\n",
    )
    .unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let path = dir.path().join("new.txt");
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let open = path.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(open, window, cx)));
    cx.run_until_parked();
    let editor = workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().expect("a tab"));
    cx.simulate_input("a\nb");
    save(&editor, cx);
    assert_eq!(std::fs::read(&path).unwrap(), b"\xEF\xBB\xBFa\r\nb");
}

#[gpui::test]
fn a_bom_file_under_both_settings_converts_both(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (editor, cx) = opened(
        cx,
        dir.path(),
        "root = true\n[*]\ncharset = utf-8\nend_of_line = crlf\n",
        "a.txt",
        b"\xEF\xBB\xBFone\n",
    );
    assert!(!dirty(&editor, cx));
    cx.simulate_input("x");
    save(&editor, cx);
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        b"xone\r\n"
    );
}

#[gpui::test]
fn removing_end_of_line_drops_the_pending_conversion(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (editor, cx) = opened(
        cx,
        dir.path(),
        "root = true\n[*]\nend_of_line = crlf\n",
        "a.txt",
        b"one\n",
    );
    std::fs::write(dir.path().join(".editorconfig"), "root = true\n").unwrap();
    let changed = vec![fe_project::FsChange::Path(dir.path().join(".editorconfig"))];
    cx.update(|_, cx| crate::pane::editorconfigs_changed(&changed, cx));
    cx.run_until_parked();
    assert!(!dirty(&editor, cx));
    cx.simulate_input("x");
    save(&editor, cx);
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"xone\n");
}
