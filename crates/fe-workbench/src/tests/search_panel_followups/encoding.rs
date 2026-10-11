//! Highlights and selections in files the editor decodes: a UTF-8 BOM and a
//! legacy encoding.

use std::ops::Range;

use fe_project::{FileMatches, LineMatch};
use gpui::{Modifiers, TestAppContext};

use super::{editor, find, line_row, project, result_center, run, searching, shown};
use crate::search_panel::{ShownRow, decode_batch};

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

/// Searches `bytes` for "needle" as a file `a.txt` of the encoding named
/// `label`, and opens its one match: the preview shown, its highlight, and
/// the editor's text and selection.
fn search_and_open(
    cx: &mut TestAppContext,
    bytes: &[u8],
    label: &str,
) -> (String, Range<usize>, String, Range<usize>) {
    let buffer = fe_buffer::Buffer::from_reader_detecting(bytes).unwrap();
    assert_eq!(buffer.encoding().label(), label);
    let dir = project(&[("a.txt", bytes)]);
    let (workbench, search, cx) = searching(cx, dir.path());
    run("needle", cx);

    let rows = shown(&search, cx);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let ShownRow::Line(_, preview, highlights) = &rows[1] else {
        panic!("{rows:?}");
    };
    assert_eq!(highlights.len(), 1, "{rows:?}");
    let highlight = highlights[0].clone();
    let preview = preview.to_string();

    let row = result_center(&search, 1, cx);
    cx.simulate_click(row, Modifiers::none());
    let (text, selection, _) = editor(&workbench, cx);
    (preview, highlight, text, selection)
}

#[gpui::test]
fn a_legacy_line_that_is_valid_utf8_is_converted(cx: &mut TestAppContext) {
    // Line 2's bytes are valid UTF-8, "é needle", though the file is not.
    let french: &[u8] = b"Les \xE9l\xE8ves ont \xE9tudi\xE9 \xE0 l'\xE9cole fran\xE7aise.\n";
    let bytes = &[french, b"\xC3\xA9 needle\n"].concat();
    let (preview, highlight, text, selection) = search_and_open(cx, bytes, "windows-1252");
    assert_eq!(preview, "Ã© needle");
    assert_eq!(&preview[highlight], "needle");
    assert!(text.ends_with("\nÃ© needle\n"), "{text:?}");
    assert_eq!(&text[selection], "needle");
}

/// "needle", long Shift_JIS text, in a line of its own.
fn sjis(before: &[u8], after: &[u8]) -> Vec<u8> {
    let mut bytes = before.to_vec();
    bytes.extend_from_slice(b"needle");
    bytes.extend_from_slice(after);
    bytes.push(b'\n');
    bytes
}

#[gpui::test]
fn a_preview_starting_inside_a_legacy_char_is_widened(cx: &mut TestAppContext) {
    // The preview, of lossy units 3 bytes each, starts at byte 433, inside
    // the char at 432..434.
    let bytes = [&b"\x82\xA0".repeat(300)[..], b" ", &sjis(b"", b"!!")].concat();
    let (preview, highlight, text, selection) = search_and_open(cx, &bytes, "Shift_JIS");
    assert_eq!(&text[selection.clone()], "needle");
    assert_eq!(selection.start, 901);
    assert_eq!(&preview[highlight], "needle");
    assert!(
        text.contains(&preview) && !preview.contains('\u{FFFD}'),
        "{preview:?}"
    );
}

#[gpui::test]
fn a_preview_ending_inside_a_legacy_char_is_widened(cx: &mut TestAppContext) {
    // Lossy units of 3 bytes after the 9 before them make the preview end at
    // byte 176, inside the char at 175..177.
    let bytes = sjis(b"", &[b"!!!".as_slice(), &b"\x82\xA0".repeat(300)].concat());
    let (preview, highlight, text, selection) = search_and_open(cx, &bytes, "Shift_JIS");
    assert_eq!(&text[selection], "needle");
    assert_eq!(&preview[highlight], "needle");
    assert!(
        text.contains(&preview) && !preview.contains('\u{FFFD}'),
        "{preview:?}"
    );
}

/// What `decode_batch` makes of one line of a Shift_JIS file: `line` after
/// a first line of valid Shift_JIS past the 64 KiB the encoding is detected from.
fn decoded_sjis(line: &[u8], ranges: Vec<Range<usize>>, preview: &str) -> LineMatch {
    let first = [
        &b"\x82\xB1\x82\xF1\x82\xC9\x82\xBF\x82\xCD\x90\xA2\x8A\xAB".repeat(5000)[..],
        b"\n",
    ]
    .concat();
    let dir = project(&[("a.txt", &[&first[..], line].concat())]);
    let buffer = fe_buffer::Buffer::from_reader_detecting(&*[&first[..], line].concat()).unwrap();
    assert_eq!(buffer.encoding().label(), "Shift_JIS");
    let preview_start = 0;
    let mut batch = vec![FileMatches {
        path: dir.path().join("a.txt"),
        rel: "a.txt".into(),
        lines: vec![LineMatch {
            line_number: 2,
            line_offset: first.len() as u64,
            ranges,
            preview: preview.into(),
            preview_start,
        }],
    }];
    decode_batch(&mut batch, &|| false).unwrap();
    batch.remove(0).lines.remove(0)
}

#[test]
fn a_malformed_lead_before_ascii_ends_before_the_ascii() {
    // \x82 then a space: U+FFFD, then the space.
    let line = decoded_sjis(b"\x82 needle", vec![0..1, 1..8], "\u{FFFD} needle");
    assert_eq!(line.ranges, [0..3, 3..10]);
    assert_eq!(line.preview, "\u{FFFD} needle");
}

#[test]
fn a_trailing_incomplete_char_is_a_replacement_at_the_line_end() {
    // "needle" and a lead byte with nothing after it: U+FFFD, 3 bytes.
    let ranges = vec![0..6, 6..7, 7..7, 5..6];
    let line = decoded_sjis(b"needle\x82", ranges, "needle\u{FFFD}");
    assert_eq!(line.ranges, [0..6, 6..9, 9..9, 5..6]);
    assert_eq!(line.preview, "needle\u{FFFD}");
}
