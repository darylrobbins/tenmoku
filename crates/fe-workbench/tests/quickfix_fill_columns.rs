//! The columns of `:vimgrep`'s items on a line read from disk (a line over
//! 512 bytes, a byte order mark), through the public `:vimgrep` path.

use std::{fs, path::Path};

use fe_gpui::{EditorSettings, EditorTheme, set_vim_enabled};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn window(
    cx: &mut TestAppContext,
    root: &Path,
    name: &str,
) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join(name);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// The (row, column) the caret lands on after `:vimgrep /x/g %` and `next`
/// `:cnext`s, in the file `name` holding `bytes`.
fn caret_after(name: &str, bytes: &[u8], next: usize) -> (u32, u32) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(name), bytes).unwrap();
    let mut cx = TestAppContext::single();
    let (workbench, mut vcx) = window(&mut cx, dir.path(), name);
    let cx = &mut vcx;
    let mut ex = |line: &str| {
        cx.simulate_keystrokes(":");
        cx.simulate_input(line);
        cx.simulate_keystrokes("enter");
        draw(cx);
    };
    ex("vimgrep /x/g %");
    for _ in 0..next {
        ex("cnext");
    }
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        let point = model.snapshot().text().offset_to_point(head);
        (point.row, point.column)
    })
}

#[test]
fn a_match_past_512_bytes_gets_its_column() {
    // The first match starts the line, so the preview starts it too, but
    // the second is past the preview's 512 bytes.
    let line = format!("x{}x\n", "a".repeat(600));
    assert_eq!(caret_after("a.txt", line.as_bytes(), 1), (0, 601));
}

#[test]
fn a_bom_file_gets_the_right_column() {
    let text = "\u{feff}ab x\n";
    assert_eq!(caret_after("a.txt", text.as_bytes(), 0), (0, 3));
    let long = format!("\u{feff}{} x\n", "a".repeat(600));
    assert_eq!(caret_after("a.txt", long.as_bytes(), 0), (0, 601));
}
