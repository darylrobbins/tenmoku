//! Highlights and selections in files the editor decodes: a UTF-8 BOM and a
//! legacy encoding.

use gpui::{Modifiers, TestAppContext};

use super::{editor, find, line_row, project, result_center, run, searching, shown};
use crate::search_panel::ShownRow;

#[gpui::test]
fn a_match_in_a_bom_file_selects_the_word(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", b"\xEF\xBB\xBFhello needle\n")]);
    let (workbench, search, cx) = searching(cx, dir.path());
    run("needle", cx);

    let rows = shown(&search, cx);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[1], line_row(1, "hello needle", &[(6, 12)]));

    let row = result_center(&search, 1, cx);
    cx.simulate_click(row, Modifiers::none());
    let (text, selection, _) = editor(&workbench, cx);
    assert_eq!(text, "hello needle\n");
    assert_eq!(selection, 6..12);
    assert_eq!(&text[selection], "needle");
}

#[gpui::test]
fn a_match_in_a_windows_1252_file_selects_the_word(cx: &mut TestAppContext) {
    let bytes: &[u8] = b"caf\xE9 na\xEFve needle\n";
    let buffer = fe_buffer::Buffer::from_reader_detecting(bytes).unwrap();
    assert_eq!(buffer.encoding().label(), "windows-1252");
    let dir = project(&[("a.txt", bytes)]);
    let (workbench, search, cx) = searching(cx, dir.path());
    run("needle", cx);

    let rows = shown(&search, cx);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let ShownRow::Line(1, preview, highlights) = &rows[1] else {
        panic!("{rows:?}");
    };
    assert_eq!(preview, "café naïve needle");
    assert_eq!(highlights, &[find(preview, "needle")]);

    let row = result_center(&search, 1, cx);
    cx.simulate_click(row, Modifiers::none());
    let (text, selection, _) = editor(&workbench, cx);
    assert_eq!(text, "café naïve needle\n");
    assert_eq!(&text[selection], "needle");
}
