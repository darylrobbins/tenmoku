//! A `.editorconfig` `charset` picks the encoding new files save in: a tab
//! for a missing file, and an untitled tab saved under a name.

use std::path::Path;

use fe_buffer::{Encoding, FileEncoding};
use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Workbench, tests::open_root};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    charset: &str,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    std::fs::write(
        root.join(".editorconfig"),
        format!("root = true\n[*]\ncharset = {charset}\n"),
    )
    .unwrap();
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();
    (workbench, cx)
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().expect("a tab"))
}

fn encoding(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> FileEncoding {
    editor.read_with(cx, |editor, _| editor.model().buffer().encoding())
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

/// A new untitled tab holding `text`.
fn untitled(
    workbench: &Entity<Workbench>,
    text: &str,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    cx.simulate_keystrokes("cmd-n");
    cx.run_until_parked();
    let editor = active(workbench, cx);
    assert!(editor.read_with(cx, |e, _| e.model().path().is_none()));
    cx.simulate_input(text);
    editor
}

fn save_as(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    path: &Path,
    cx: &mut VisualTestContext,
) {
    let path = path.to_owned();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.save_editor_as(editor, path, window, cx)
    });
    cx.run_until_parked();
}

#[gpui::test]
fn new_files_take_the_charset(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let utf16 = tempfile::tempdir().unwrap();
    let (workbench, cx) = setup(cx, utf16.path(), "utf-16le");

    // A missing file under utf-16le.
    let new = utf16.path().join("new.txt");
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let path = new.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
    let editor = active(&workbench, cx);
    assert_eq!(
        editor.read_with(cx, |e, _| e.model().path().map(Path::to_owned)),
        Some(new.clone())
    );
    let le = FileEncoding {
        encoding: Encoding::for_label(b"utf-16le").unwrap(),
        bom: true,
    };
    assert_eq!(encoding(&editor, cx), le);
    assert!(!dirty(&editor, cx), "untouched, it needs no saving");
    assert!(!new.exists());
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_input("é");
    assert!(dirty(&editor, cx));
    let saved = cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
    cx.run_until_parked();
    drop(saved);
    assert_eq!(std::fs::read(&new).unwrap(), [0xFF, 0xFE, 0xE9, 0x00]);

    // An untitled tab saved under latin1.
    std::fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\ncharset = latin1\n",
    )
    .unwrap();
    let editor = untitled(&workbench, "é", cx);
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    let latin1 = dir.path().join("latin1.txt");
    save_as(&workbench, &editor, &latin1, cx);
    assert_eq!(std::fs::read(&latin1).unwrap(), [0xE9]);
    assert_eq!(encoding(&editor, cx).name(), "windows-1252");
    assert!(!dirty(&editor, cx));

    // One latin1 can't hold: refused, and back in UTF-8.
    let editor = untitled(&workbench, "😀", cx);
    let emoji = dir.path().join("emoji.txt");
    save_as(&workbench, &editor, &emoji, cx);
    assert!(!emoji.exists());
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    assert!(editor.read_with(cx, |e, _| e.model().path().is_none()));
    assert!(dirty(&editor, cx));
}

#[gpui::test]
fn a_new_file_saved_first_keeps_its_encoding(cx: &mut TestAppContext) {
    // Saved as UTF-8 with no charset, then a charset is added.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".editorconfig"), "root = true\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let new = dir.path().join("new.txt");
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let path = new.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
    let editor = active(&workbench, cx);
    editor.update(cx, |e, _| {
        e.model_mut().insert("é");
        e.model_mut().save().unwrap();
    });
    std::fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\ncharset = latin1\n",
    )
    .unwrap();
    let changed = vec![fe_project::FsChange::Path(dir.path().join(".editorconfig"))];
    cx.update(|_, cx| crate::pane::editorconfigs_changed(&changed, cx));
    cx.run_until_parked();
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    assert!(!dirty(&editor, cx));
    assert_eq!(std::fs::read(&new).unwrap(), "é".as_bytes());
}

#[gpui::test]
fn a_new_file_saved_before_its_properties_keeps_its_encoding(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\ncharset = utf-16le\n",
    )
    .unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    // A missing file's read lands, and its tab is made; its properties'
    // read starts.
    let new = dir.path().join("new.txt");
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let path = new.clone();
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.file_read(path, Ok(crate::loader::Opened::Text(None)), window, cx)
        })
    });
    let editor = active(&workbench, cx);
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    // Saved before the properties' read has landed.
    editor.update(cx, |e, _| {
        e.model_mut().insert("é");
        e.model_mut().save().unwrap();
    });
    cx.run_until_parked();
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    assert!(!dirty(&editor, cx));
    assert_eq!(std::fs::read(&new).unwrap(), "é".as_bytes());
}

/// Three folders with `.editorconfig` charsets (none if empty).
fn folders(charsets: [&str; 3]) -> Vec<tempfile::TempDir> {
    charsets
        .iter()
        .map(|charset| {
            let dir = tempfile::tempdir().unwrap();
            let config = match *charset {
                "" => "root = true\n".to_owned(),
                charset => format!("root = true\n[*]\ncharset = {charset}\n"),
            };
            std::fs::write(dir.path().join(".editorconfig"), config).unwrap();
            dir
        })
        .collect()
}

/// Asks for a Save As of `editor` into each of `dirs` at once, then lets
/// them run.
fn save_as_queued(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    dirs: &[tempfile::TempDir],
    cx: &mut VisualTestContext,
) -> Vec<std::path::PathBuf> {
    let paths: Vec<_> = dirs.iter().map(|dir| dir.path().join("a.txt")).collect();
    for path in &paths {
        let path = path.clone();
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.save_editor_as(editor, path, window, cx)
        });
    }
    cx.run_until_parked();
    paths
}

#[gpui::test]
fn queued_save_as_takes_the_charset_while_untitled(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, root.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));

    // The first has no charset: the queued ones, which find the buffer at
    // a path once it's done, save in its UTF-8, never in their own charset.
    let dirs = folders(["", "latin1", "utf-16le"]);
    let editor = untitled(&workbench, "é", cx);
    let paths = save_as_queued(&workbench, &editor, &dirs, cx);
    for path in &paths {
        assert_eq!(std::fs::read(path).unwrap(), "é".as_bytes(), "{path:?}");
    }
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    assert!(!dirty(&editor, cx));

    // The first is latin1: the rest save in latin1, the same charset too.
    let dirs = folders(["latin1", "utf-16le", "latin1"]);
    let editor = untitled(&workbench, "é", cx);
    let paths = save_as_queued(&workbench, &editor, &dirs, cx);
    for path in &paths {
        assert_eq!(std::fs::read(path).unwrap(), [0xE9], "{path:?}");
    }
    assert_eq!(encoding(&editor, cx).name(), "windows-1252");
    assert_eq!(
        editor.read_with(cx, |e, _| e.model().path().map(Path::to_owned)),
        Some(paths[2].clone())
    );
    assert!(!dirty(&editor, cx));

    // The first fails: the buffer was never in latin1. It's still untitled
    // when the next starts, which saves in its own destination's charset.
    let dirs = folders(["latin1", "utf-16le", "latin1"]);
    let editor = untitled(&workbench, "😀", cx);
    let paths = save_as_queued(&workbench, &editor, &dirs, cx);
    assert!(!paths[0].exists());
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("😀".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(std::fs::read(&paths[1]).unwrap(), utf16);
    // At a path by then: in the buffer's encoding.
    assert_eq!(std::fs::read(&paths[2]).unwrap(), utf16);
    assert_eq!(encoding(&editor, cx).name(), "UTF-16LE");
    assert!(!dirty(&editor, cx));

    // The first fails, and the next has no charset: UTF-8 from then on.
    let dirs = folders(["latin1", "", "latin1"]);
    let editor = untitled(&workbench, "😀", cx);
    let paths = save_as_queued(&workbench, &editor, &dirs, cx);
    assert!(!paths[0].exists());
    assert_eq!(std::fs::read(&paths[1]).unwrap(), "😀".as_bytes());
    assert_eq!(std::fs::read(&paths[2]).unwrap(), "😀".as_bytes());
    assert_eq!(encoding(&editor, cx), FileEncoding::UTF8);
    assert!(!dirty(&editor, cx));
}

#[gpui::test]
fn a_save_after_queued_save_as_requests_waits_its_turn(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, root.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let dirs = folders(["latin1", "utf-16le", ""]);
    let editor = untitled(&workbench, "é", cx);
    let (b, c) = (dirs[0].path().join("b.txt"), dirs[1].path().join("c.txt"));
    for path in [&b, &c] {
        let path = path.clone();
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.save_editor_as(&editor, path, window, cx)
        });
    }
    // Typed, then saved, before the first Save As is done: the save runs
    // after both, at c.txt, which has the text typed.
    editor.update(cx, |e, _| e.model_mut().insert(" two"));
    let saved = cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
    cx.run_until_parked();
    drop(saved);
    assert_eq!(std::fs::read(&b).unwrap(), [0xE9]);
    // At a path by then, c.txt keeps the buffer's latin1.
    assert_eq!(std::fs::read(&c).unwrap(), b"\xE9 two");
    assert_eq!(
        editor.read_with(cx, |e, _| e.model().path().map(Path::to_owned)),
        Some(c.clone())
    );
    assert_eq!(encoding(&editor, cx).name(), "windows-1252");
    assert!(!dirty(&editor, cx));
}
