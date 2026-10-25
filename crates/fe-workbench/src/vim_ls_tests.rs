//! `:ls`, `:buffers` and `:files` on the status bar's line list the open
//! files in a picker, and choosing a row switches to it as `:b N` does.

use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::picker::PickerDelegate as _;

use super::super::vim_host_tests::{active, draw, ex, window};
use crate::Workbench;

/// The picker's rows' cells and its status; `None` if the vim picker isn't
/// the modal showing.
fn shown(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(Vec<Vec<String>>, Option<String>)> {
    let picker = workbench.read_with(cx, |workbench, cx| workbench.vim_pick(cx))?;
    Some(picker.read_with(cx, |picker, _| {
        let status = picker.delegate.status().map(|status| status.to_string());
        (picker.delegate.rows(), status)
    }))
}

fn row(cells: [&str; 4]) -> Vec<String> {
    cells.map(String::from).to_vec()
}

fn file_name(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| {
        fe_gpui::pane_item::file_name(editor.model().path())
    })
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Opens a, b and c (each of three lines) with b modified, a current and b
/// the alternate, and the caret on line 2 of c.
fn setup(
    cx: &mut TestAppContext,
    root: &std::path::Path,
) -> (Entity<Workbench>, VisualTestContext) {
    for file in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(file), "one\ntwo\nthree\n").unwrap();
    }
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;
    cx.simulate_keystrokes("j");
    ex("b 2", cx);
    cx.simulate_keystrokes("x");
    ex("b 1", cx);
    (workbench, vcx)
}

#[gpui::test]
fn rows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = setup(cx, dir.path());
    let cx = &mut vcx;

    for line in ["ls", "buffers", "files", "ls!"] {
        ex(line, cx);
        let (rows, status) = shown(&workbench, cx).unwrap_or_else(|| panic!(":{line}"));
        assert_eq!(status.as_deref(), Some("Buffers"), ":{line}");
        assert_eq!(
            rows,
            [
                row(["1", "%a", "a.txt", "line 1"]),
                row(["2", "#h+", "b.txt", "line 1"]),
                row(["3", "h", "c.txt", "line 2"]),
            ],
            ":{line}"
        );
        cx.simulate_keystrokes("escape escape");
        assert!(shown(&workbench, cx).is_none());
    }
}

#[gpui::test]
fn choose(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = setup(cx, dir.path());
    let cx = &mut vcx;

    // Dismissing changes nothing and focuses the editor.
    ex("ls", cx);
    assert!(shown(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    assert!(shown(&workbench, cx).is_none());
    let editor = active(&workbench, cx);
    assert_eq!(file_name(&editor, cx), "a.txt");
    assert!(focused(&editor, cx));

    // Choosing the third row runs `:b 3`.
    ex("ls", cx);
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    assert!(shown(&workbench, cx).is_none());
    let editor = active(&workbench, cx);
    assert_eq!(file_name(&editor, cx), "c.txt");
    assert!(focused(&editor, cx));
}

#[gpui::test]
fn fuzzy(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = setup(cx, dir.path());
    let cx = &mut vcx;

    ex("ls", cx);
    cx.simulate_input("c.t");
    let (rows, _) = shown(&workbench, cx).unwrap();
    assert_eq!(rows, [row(["3", "h", "c.txt", "line 2"])]);
}

/// A window with no project folder lists the files by their whole paths,
/// which the picker filters by.
#[gpui::test]
fn without_a_root(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    for file in ["qz.txt", "wv.txt"] {
        std::fs::write(root.join(file), "one\n").unwrap();
    }
    let launch = crate::Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for file in ["qz.txt", "wv.txt"] {
        let path = root.join(file);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);

    ex("ls", cx);
    let (rows, _) = shown(&workbench, cx).unwrap();
    let name = |file: &str| root.join(file).display().to_string();
    assert_eq!(
        rows,
        [
            row(["1", "h", "[No Name]", "line 1"]),
            row(["2", "#h", &name("qz.txt"), "line 1"]),
            row(["3", "%a", &name("wv.txt"), "line 1"]),
        ]
    );
    cx.simulate_input("qz.t");
    let (rows, _) = shown(&workbench, cx).unwrap();
    assert_eq!(rows, [row(["2", "#h", &name("qz.txt"), "line 1"])]);
}

/// A large file open in a viewer is listed with its caret's line.
#[gpui::test]
fn viewer_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    crate::loader::test_support::lower_threshold(&root, 64 * 1024, false);
    let big: String = (0..20_000).map(|row| format!("line {row:06}\n")).collect();
    std::fs::write(root.join("big.log"), big).unwrap();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    let (workbench, mut vcx) = window(cx, &root, &["big.log"]);
    let cx = &mut vcx;
    let viewer = workbench
        .read_with(cx, |workbench, cx| {
            workbench.pane().read(cx).tabs()[0].viewer().cloned()
        })
        .expect("big.log opens in a viewer");
    for _ in 0..200 {
        let rows = viewer.read_with(cx, |viewer, _| viewer.model().text().row_count());
        if rows > 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        draw(cx);
    }
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.simulate_keystrokes("down down down");
    draw(cx);
    let head = viewer.read_with(cx, |viewer, _| viewer.model().head());
    assert!(head > 0, "the keys moved the viewer's caret");

    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    ex("ls", cx);
    let (rows, _) = shown(&workbench, cx).unwrap();
    assert_eq!(
        rows,
        [
            row(["1", "#h", "big.log", "line 4"]),
            row(["2", "%a", "a.txt", "line 1"]),
        ]
    );
}
