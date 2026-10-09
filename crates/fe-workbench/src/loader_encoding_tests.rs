//! Files open in their detected encoding, and the status bar names it.

use std::path::PathBuf;

use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, StatusItem, StatusSide, Workbench};

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn encoding(workbench: &gpui::Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Right))
        .into_iter()
        .find(|(id, _)| id == StatusItem::ENCODING)
        .map(|(_, text)| text.to_string())
        .expect("the encoding item is drawn")
}

fn setup<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (gpui::Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

fn open(workbench: &gpui::Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn text(workbench: &gpui::Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

#[gpui::test]
fn opens_in_the_detected_encoding(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("hello\n".encode_utf16().flat_map(u16::to_le_bytes));
    let cases: [(&str, Vec<u8>, &str, &str); 4] = [
        (
            "a.txt",
            b"caf\xE9 na\xEFve\n".to_vec(),
            "café naïve\n",
            "windows-1252",
        ),
        (
            "b.txt",
            b"\x82\xB1\x82\xEA\x82\xCD\x93\xFA\x96\x7B\x8C\xEA\x82\xCC\x83\x65\x83\x4C\x83\x58\x83\x67\x82\xC5\x82\xB7\x81\x42\n".to_vec(),
            "これは日本語のテキストです。\n",
            "Shift_JIS",
        ),
        ("c.txt", utf16, "hello\n", "UTF-16LE"),
        (
            "d.txt",
            b"\xEF\xBB\xBFhello\n".to_vec(),
            "hello\n",
            "UTF-8 with BOM",
        ),
    ];
    let (workbench, cx) = setup(cx, &dir);
    for (name, bytes, expected, label) in cases {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        open(&workbench, path, cx);
        let shown = text(&workbench, cx);
        assert_eq!(shown, expected, "{name}");
        assert!(!shown.contains(['\u{fffd}', '\u{feff}']), "{name}");
        assert_eq!(encoding(&workbench, cx), label, "{name}");
    }
}

#[gpui::test]
fn saves_in_the_detected_encoding(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"caf\xE9\n").unwrap();
    let (workbench, cx) = setup(cx, &dir);
    open(&workbench, path.clone(), cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));

    cx.simulate_keystrokes("cmd-right");
    cx.simulate_input("é");
    cx.simulate_keystrokes("cmd-s");
    assert_eq!(std::fs::read(&path).unwrap(), b"caf\xE9\xE9\n".to_vec());

    // An emoji has no windows-1252 byte: the save is refused.
    cx.simulate_input("😀");
    cx.simulate_keystrokes("cmd-s");
    assert_eq!(std::fs::read(&path).unwrap(), b"caf\xE9\xE9\n".to_vec());
    assert!(editor.read_with(cx, |editor, _| editor.model().is_dirty()));
}
