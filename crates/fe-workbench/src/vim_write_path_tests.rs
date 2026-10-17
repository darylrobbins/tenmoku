//! `:w {path}` and `:sav {path}` on the status bar's line.

use std::path::Path;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::TestAppContext;

use super::super::vim_host_tests::{active, dirty, draw, ex, message, window, write};

const E13: &str = "E13: File exists (add ! to override)";

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[gpui::test]
fn write_a_copy(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    cx.simulate_keystrokes("x");
    assert!(dirty(&a, cx));

    // A copy: the tab keeps its path and stays dirty.
    ex("w other.txt", cx);
    assert_eq!(read(&root.join("other.txt")), "ne\n");
    assert_eq!(read(&root.join("a.txt")), "one\n");
    assert!(dirty(&a, cx));
    assert_eq!(
        a.read_with(cx, |a, _| a.model().path().map(Path::to_owned)),
        Some(root.join("a.txt"))
    );

    // An existing file without !: E13, and it's left as it was.
    std::fs::write(root.join("other.txt"), "mine\n").unwrap();
    ex("w other.txt", cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(E13));
    assert_eq!(read(&root.join("other.txt")), "mine\n");

    // With ! it's overwritten.
    ex("w! other.txt", cx);
    assert_eq!(read(&root.join("other.txt")), "ne\n");
    assert!(dirty(&a, cx));
}

#[gpui::test]
fn save_as(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;

    // :sav on a named buffer renames the tab.
    let a = active(&workbench, cx);
    cx.simulate_keystrokes("x");
    ex("sav b.txt", cx);
    assert_eq!(read(&root.join("b.txt")), "ne\n");
    assert_eq!(read(&root.join("a.txt")), "one\n");
    assert!(!dirty(&a, cx));
    assert_eq!(
        a.read_with(cx, |a, _| a.model().path().map(Path::to_owned)),
        Some(root.join("b.txt"))
    );

    // :w {path} on an untitled buffer is a save as.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new("new"), None), cx);
            let ix = pane.tabs().len() - 1;
            pane.activate(ix, window, cx);
        })
    });
    draw(cx);
    let untitled = active(&workbench, cx);
    ex("w c.txt", cx);
    assert_eq!(read(&root.join("c.txt")), "new");
    assert!(!dirty(&untitled, cx));
    assert_eq!(
        untitled.read_with(cx, |e, _| e.model().path().map(Path::to_owned)),
        Some(root.join("c.txt"))
    );
}

/// The destination's `.editorconfig` fixes go into what `:sav` writes, as an
/// ordinary save writes the file's own: at once, and queued behind a save.
#[gpui::test]
fn save_as_applies_the_destination_editorconfig(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    std::fs::write(
        root.join(".editorconfig"),
        "root = true\n[b.txt]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n[c.txt]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
    )
    .unwrap();
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    draw(cx);
    let type_dirty = |cx: &mut gpui::VisualTestContext| {
        a.update(cx, |a, _| {
            a.model_mut().select_all();
            a.model_mut().insert("a  \nb\t");
        });
    };

    // a.txt has no fixes, so an ordinary save writes the text as typed.
    type_dirty(cx);
    ex("w", cx);
    assert_eq!(read(&root.join("a.txt")), "a  \nb\t");
    let ordinary = "a\nb\n";

    // Queued behind a running save, which (a.txt has no fixes) writes the
    // text as typed: Save As still normalises for c.txt.
    type_dirty(cx);
    let saving = cx.update(|window, cx| {
        a.update(cx, |a, cx| {
            a.set_save_hook(Some(std::rc::Rc::new(HoldSave)));
            a.save(window, cx)
        })
    });
    cx.run_until_parked();
    ex("sav c.txt", cx);
    assert!(!root.join("c.txt").exists());
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    drop(saving);
    assert_eq!(read(&root.join("a.txt")), "a  \nb\t");
    assert_eq!(read(&root.join("c.txt")), ordinary);

    // At once, from a source without fixes: d.txt keeps the text as typed,
    // b.txt gets the destination's.
    a.update(cx, |a, _| a.set_save_hook(None));
    type_dirty(cx);
    ex("sav! d.txt", cx);
    assert_eq!(read(&root.join("d.txt")), "a  \nb\t");
    type_dirty(cx);
    ex("sav b.txt", cx);
    assert_eq!(read(&root.join("b.txt")), ordinary);
}

struct HoldSave;

impl fe_gpui::pane_item::SaveHook for HoldSave {
    fn before_save(
        &self,
        _: &gpui::Entity<fe_gpui::EditorView>,
        cx: &mut gpui::App,
    ) -> gpui::Task<()> {
        gpui::AppContext::background_spawn(cx, std::future::pending())
    }
}
