//! A vim map that runs a file command (`nnoremap X :w<CR>dd`) goes on once
//! the workbench has run it, and ends if it failed.

use fe_gpui::set_vim_remaps;
use fe_vim::{Remap, RemapMode};
use gpui::TestAppContext;

use super::super::vim_host_tests::{active, dirty, draw, window, write};

fn map_x(cx: &mut gpui::VisualTestContext) {
    let map = Remap {
        modes: vec![RemapMode::Normal],
        from: "X".into(),
        to: ":w<CR>dd".into(),
        recursive: false,
    };
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &[map], ","));
    assert_eq!(issues, []);
    draw(cx);
}

fn text(editor: &gpui::Entity<fe_gpui::EditorView>, cx: &mut gpui::VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

/// The write saves the file before `dd` deletes the line, and the buffer
/// stays dirty with the delete.
#[gpui::test]
fn mapped_write_saves_then_deletes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    map_x(cx);
    cx.simulate_keystrokes("x");
    assert!(dirty(&a, cx));
    cx.simulate_keystrokes("shift-x");
    draw(cx);
    // The saved file has the line `dd` then deleted.
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "ne\n");
    assert_eq!(text(&a, cx), "");
    assert!(dirty(&a, cx));
}

/// A write that can't be done ends the map before the delete.
#[gpui::test]
fn mapped_failed_write_keeps_the_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    map_x(cx);
    cx.simulate_keystrokes("x");
    // The file's folder is gone, so no save can be made, whoever runs this.
    std::fs::remove_dir_all(root).unwrap();
    cx.simulate_keystrokes("shift-x");
    draw(cx);
    assert_eq!(text(&a, cx), "ne\n");
    assert!(dirty(&a, cx));
}

/// A quit ends the map where it closes the tab: the keys after it aren't run
/// on the closed editor, clean or not.
#[gpui::test]
fn mapped_quit_ends_the_map(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let b = active(&workbench, cx);
    for (from, to) in [("Q", ":q<CR>dd"), ("Z", ":x<CR>dd")] {
        let map = Remap {
            modes: vec![RemapMode::Normal],
            from: from.into(),
            to: to.into(),
            recursive: false,
        };
        cx.update(|_, cx| set_vim_remaps(cx, &[map], ","));
        draw(cx);
        let key = format!("shift-{}", from.to_lowercase());
        cx.simulate_keystrokes(&key);
        draw(cx);
        assert_eq!(text(&b, cx), "one\n", "{to}");
        // Reopened for the next map.
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let path = root.join("b.txt");
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
}

/// `:e` opens its file on its own time, so a map ends there instead of
/// running its keys on the editor it left.
#[gpui::test]
fn mapped_edit_ends_the_map(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    let map = Remap {
        modes: vec![RemapMode::Normal],
        from: "Y".into(),
        to: ":e b.txt<CR>dd".into(),
        recursive: false,
    };
    cx.update(|_, cx| set_vim_remaps(cx, &[map], ","));
    draw(cx);
    cx.simulate_keystrokes("shift-y");
    draw(cx);
    assert_eq!(text(&a, cx), "one\n");
}

/// `:wq` ends the map once the tab is closed.
#[gpui::test]
fn mapped_write_quit_waits_for_the_close(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let b = active(&workbench, cx);
    let map = Remap {
        modes: vec![RemapMode::Normal],
        from: "Z".into(),
        to: ":wq<CR>dd".into(),
        recursive: false,
    };
    cx.update(|_, cx| set_vim_remaps(cx, &[map], ","));
    draw(cx);
    cx.simulate_keystrokes("x shift-z");
    draw(cx);
    assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "ne\n");
    // The tab is gone, and the line `dd` would delete is the buffer's own.
    assert_eq!(text(&b, cx), "ne\n");
}
