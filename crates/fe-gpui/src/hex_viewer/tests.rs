//! A [`HexViewer`] over generated files, in a headless window, with keys
//! through the real keymap: selecting in either column, the keys, a 5 GiB
//! file, Go to Offset and Copy.

use std::{cell::RefCell, path::Path, rc::Rc, sync::Arc};

use fe_command::Invocation;
use fe_fileview::{CHUNK_BYTES, FileView};
use gpui::{
    ClipboardItem, Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, VisualTestContext, point, px,
};
use tempfile::TempDir;

use super::*;

pub(super) type Events = Rc<RefCell<Vec<HexViewerEvent>>>;

/// The window's root, which makes the viewer and subscribes to it at once,
/// as a pane would.
pub(super) struct Host {
    pub(super) viewer: Entity<HexViewer>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.viewer.clone()
    }
}

/// `data.bin` in a new tempdir, holding `bytes`.
pub(super) fn file_of(bytes: &[u8]) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

/// A focused viewer of `path` in a headless window, drawn, and the events
/// it sends.
pub(super) fn viewer<'a>(
    cx: &'a mut TestAppContext,
    path: &Path,
) -> (Entity<HexViewer>, &'a mut VisualTestContext, Events) {
    let view = Arc::new(FileView::open(path).unwrap());
    cx.update(|cx| crate::init_with_keymap(cx, host_keymap()));
    let events = Events::default();
    let heard = events.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let theme = EditorTheme::default_dark(cx);
        let viewer = cx.new(|cx| HexViewer::new(view, theme, cx));
        cx.subscribe(&viewer, move |_, _, event: &HexViewerEvent, _| {
            heard.borrow_mut().push(event.clone())
        })
        .detach();
        Host { viewer }
    });
    let viewer = host.read_with(cx, |host, _| host.viewer.clone());
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.run_until_parked();
    draw(cx);
    (viewer, cx, events)
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The centre of byte `offset`'s cell in `column`, which must be on screen.
pub(super) fn cell(
    viewer: &Entity<HexViewer>,
    cx: &mut VisualTestContext,
    column: Column,
    offset: u64,
) -> Point<Pixels> {
    viewer.read_with(cx, |viewer, _| {
        let layout = viewer.layout.expect("drawn");
        let columns = Columns::for_len(viewer.view.len());
        let row = offset / ROW_BYTES - viewer.top_row;
        let cell = columns.cell(column, (offset % ROW_BYTES) as usize);
        let x = layout.x(cell.start) + layout.char_width * ((cell.end - cell.start) as f32 / 2.);
        let y = layout.rows.top() - viewer.top_px + layout.row_height * (row as f32 + 0.5);
        point(x, y)
    })
}

pub(super) fn selected(viewer: &Entity<HexViewer>, cx: &mut VisualTestContext) -> Range<u64> {
    viewer.read_with(cx, |viewer, _| viewer.selected())
}

/// The cells the last frame filled, as (column, offset, colour).
fn highlighted(viewer: &Entity<HexViewer>, cx: &mut VisualTestContext) -> Vec<(Column, u64, Hsla)> {
    viewer.read_with(cx, |viewer, _| viewer.highlighted.clone())
}

/// The built-in keymap resolved for the host, as the app runs it: ctrl keys on
/// Linux, cmd on macOS, so the keystrokes below are the real ones.
pub(super) fn host_keymap() -> fe_keymap::Keymap {
    let (layer, diagnostics) = fe_keymap::KeymapLayer::parse_for(
        std::path::Path::new(fe_keymap::DEFAULT_KEYMAP_PATH),
        fe_keymap::DEFAULT,
        None,
        kiln_core::Os::current(),
    );
    assert_eq!(diagnostics, []);
    fe_keymap::Keymap::from_layers(vec![layer.unwrap()])
}

/// The secondary modifier's key name on this platform.
pub(super) fn secondary() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

/// Runs command `id` from the focused element, as a binding would.
pub(super) fn run(cx: &mut VisualTestContext, id: &str) {
    cx.dispatch_action(RunCommand(Invocation::of(id).unwrap()));
}

/// The bytes 0, 1, … 255, 0, 1, … of a file `len` long.
pub(super) fn counting(len: usize) -> Vec<u8> {
    (0..len).map(|i| i as u8).collect()
}

#[gpui::test]
fn dragging_in_hex_highlights_the_same_bytes_in_ascii(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(256));
    let (viewer, cx, _) = viewer(cx, &path);
    let theme = viewer.read_with(cx, |viewer, _| viewer.theme.clone());

    let from = cell(&viewer, cx, Column::Hex, 2);
    let to = cell(&viewer, cx, Column::Hex, 20);
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);

    assert_eq!(selected(&viewer, cx), 2..21);
    assert_eq!(viewer.read_with(cx, |v, _| v.column()), Column::Hex);
    let mut expected = Vec::new();
    for at in 2..21 {
        expected.push((Column::Hex, at, theme.selection));
        expected.push((Column::Ascii, at, theme.identifier_highlight));
    }
    assert_eq!(highlighted(&viewer, cx), expected);
    assert_ne!(theme.selection, theme.identifier_highlight);
}

#[gpui::test]
fn clicking_ascii_selects_the_byte_in_hex(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(b"The quick brown fox jumps over the lazy dog, twice over.");
    let (viewer, cx, _) = viewer(cx, &path);
    let theme = viewer.read_with(cx, |viewer, _| viewer.theme.clone());

    let at = cell(&viewer, cx, Column::Ascii, 37);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(selected(&viewer, cx), 37..38);
    assert_eq!(viewer.read_with(cx, |v, _| v.column()), Column::Ascii);
    assert_eq!(
        highlighted(&viewer, cx),
        [
            (Column::Hex, 37, theme.identifier_highlight),
            (Column::Ascii, 37, theme.selection),
        ]
    );

    // Shift-click extends, from either column; the last one clicked has it.
    let to = cell(&viewer, cx, Column::Hex, 40);
    cx.simulate_click(to, Modifiers::shift());
    draw(cx);
    assert_eq!(selected(&viewer, cx), 37..41);
    assert_eq!(viewer.read_with(cx, |v, _| v.column()), Column::Hex);
    assert_eq!(viewer.read_with(cx, |v, _| v.selection_ends()), (37, 40));
}

#[gpui::test]
fn shift_arrows_extend_by_byte_and_row(cx: &mut TestAppContext) {
    // 100 rows, the last one partial.
    let len = 16 * 99 + 5;
    let (_dir, path) = file_of(&counting(len));
    let (viewer, cx, _) = viewer(cx, &path);
    let ends = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.selection_ends());
    let secondary = secondary();

    cx.simulate_keystrokes("shift-right shift-right");
    assert_eq!(selected(&viewer, cx), 0..3);
    cx.simulate_keystrokes("shift-down");
    assert_eq!(selected(&viewer, cx), 0..19);
    cx.simulate_keystrokes("shift-left shift-up");
    assert_eq!(ends(cx), (0, 1));
    cx.simulate_keystrokes("right down");
    assert_eq!(ends(cx), (18, 18));
    cx.simulate_keystrokes("end");
    assert_eq!(ends(cx), (31, 31));
    cx.simulate_keystrokes("shift-home");
    assert_eq!(selected(&viewer, cx), 16..32);
    cx.simulate_keystrokes("left up up");
    assert_eq!(ends(cx), (15, 15));

    // A page, then the file's ends, scrolled into view.
    draw(cx);
    let page = viewer.read_with(cx, |v, _| v.page_rows());
    assert!(page > 2 && page < 90, "{page}");
    cx.simulate_keystrokes("pagedown");
    assert_eq!(ends(cx), (15 + 16 * page, 15 + 16 * page));
    cx.simulate_keystrokes(&format!("{secondary}-down"));
    let last = len as u64 - 1;
    assert_eq!(ends(cx), (last, last));
    draw(cx);
    let (top, painted) = viewer.read_with(cx, |v, _| (v.top_row, v.painted.clone()));
    assert_eq!(top, 100 - page);
    assert_eq!(painted.last().map(|row| row.row), Some(99));
    // Down from the row above the partial one stops at its last byte.
    cx.simulate_keystrokes("up up down down");
    assert_eq!(ends(cx), (last, last));
    cx.simulate_keystrokes(&format!("shift-{secondary}-up"));
    assert_eq!(selected(&viewer, cx), 0..len as u64);
    cx.simulate_keystrokes(&format!("{secondary}-up"));
    assert_eq!(ends(cx), (0, 0));
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.top_row), 0);
}

#[gpui::test]
fn typing_edits_nothing(cx: &mut TestAppContext) {
    let bytes = counting(64);
    let (_dir, path) = file_of(&bytes);
    let (viewer, cx, events) = viewer(cx, &path);
    cx.simulate_keystrokes("right shift-right");
    let before = viewer.read_with(cx, |v, _| v.selection_ends());

    cx.simulate_input("abc");
    cx.simulate_keystrokes("x 0 space backspace delete enter tab");
    cx.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
    cx.simulate_keystrokes(&format!("{}-v", secondary()));
    draw(cx);

    assert_eq!(viewer.read_with(cx, |v, _| v.selection_ends()), before);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(*events.borrow(), []);
    let first = viewer.read_with(cx, |v, _| v.painted[0].text.clone());
    assert!(first.starts_with("00000000  00 01 02 03"), "{first}");
}

#[gpui::test]
fn a_5_gib_sparse_file_scrolls_to_its_last_row_reading_only_the_screen(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sparse.bin");
    let len: u64 = 5 << 30;
    std::fs::File::create(&path).unwrap().set_len(len).unwrap();
    let (viewer, cx, events) = viewer(cx, &path);

    let (cached, first) =
        viewer.read_with(cx, |v, _| (v.view.cached_bytes(), v.painted[0].clone()));
    assert!(cached <= 2 * CHUNK_BYTES, "{cached}");
    assert_eq!(first.row, 0);
    // Past 4 GiB, offsets take 16 digits.
    assert!(
        first.text.starts_with("0000000000000000  00 00"),
        "{}",
        first.text
    );

    cx.simulate_keystrokes(&format!("{}-down", secondary()));
    draw(cx);
    let last_row = len / 16 - 1;
    let (painted, top, page) =
        viewer.read_with(cx, |v, _| (v.painted.clone(), v.top_row, v.page_rows()));
    assert_eq!(selected(&viewer, cx), len - 1..len);
    assert_eq!(top, last_row + 1 - page);
    let last = painted.last().expect("rows");
    assert_eq!(last.row, last_row);
    assert!(
        last.text
            .starts_with(&format!("{:016X}  00", last_row * 16)),
        "{}",
        last.text
    );
    assert!(last.text.starts_with("000000013FFFFFF0"), "{}", last.text);
    let cached = viewer.read_with(cx, |v, _| v.view.cached_bytes());
    assert!(cached <= 4 * CHUNK_BYTES, "{cached}");

    // The wheel moves the u64 top row by whole rows, exactly.
    let row_height = viewer.read_with(cx, |v, _| v.layout.unwrap().row_height);
    let at = cell(&viewer, cx, Column::Hex, (top + 1) * 16);
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), row_height * 3.)),
        ..Default::default()
    });
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.top_row), top - 3);
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn go_to_offset_takes_decimal_and_0x(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(64 * 1024));
    let (viewer, cx, _) = viewer(cx, &path);
    let input_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let viewer = viewer.read(cx);
            let input = viewer.inputs.as_ref().map(|inputs| inputs.go_to.clone());
            input.is_some_and(|input| input.focus_handle(cx).is_focused(window))
        })
    };
    let rows_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| viewer.focus_handle(cx).is_focused(window))
    };

    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), Some(Opened::GoTo));
    assert!(input_focused(cx));
    cx.simulate_input("100");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(selected(&viewer, cx), 100..101);
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), None);
    assert!(rows_focused(cx));

    // Hex, far down: shown.
    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    cx.simulate_input("0xF00A");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(selected(&viewer, cx), 0xF00A..0xF00B);
    let (top, page) = viewer.read_with(cx, |v, _| (v.top_row, v.page_rows()));
    assert!((top..top + page).contains(&0xF00), "{top} {page}");

    // Escape closes it, keeping the selection, and the rows have focus.
    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    cx.simulate_input("12");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), None);
    assert_eq!(selected(&viewer, cx), 0xF00A..0xF00B);
    assert!(rows_focused(cx));
    // The input's text was selected on opening, so typing replaces it.
    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    cx.simulate_input("0x10");
    cx.simulate_keystrokes("enter");
    assert_eq!(selected(&viewer, cx), 0x10..0x11);
}

#[gpui::test]
fn go_to_offset_past_the_end_says_so(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(4096));
    let (viewer, cx, _) = viewer(cx, &path);
    cx.simulate_keystrokes("right right");
    let said = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |v, _| v.go_to_error().map(|said| said.to_string()))
    };

    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    cx.simulate_input("4096");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        said(cx).as_deref(),
        Some("4096 is past the end: the last byte is 0xFFF")
    );
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), Some(Opened::GoTo));
    assert_eq!(selected(&viewer, cx), 2..3);
    // Drawn in the header.
    assert!(cx.debug_bounds(verbatim!("hex-viewer-said")).is_some());

    cx.update(|window, cx| {
        let input = viewer.read(cx).inputs.as_ref().unwrap().go_to.clone();
        input.update(cx, |input, cx| input.set_text("0xZZ", cx));
        window.focus(&input.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(said(cx).as_deref(), Some("Not an offset: 0xZZ"));
    assert_eq!(selected(&viewer, cx), 2..3);

    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), None);
    assert_eq!(said(cx), None);
    assert_eq!(selected(&viewer, cx), 2..3);
    assert!(cx.update(|window, cx| viewer.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn copy_follows_the_focused_column(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(b"Hi\0\xFFthere");
    let (viewer, cx, _) = viewer(cx, &path);
    let copy = format!("{}-c", secondary());
    let copied = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|item| item.text());

    let (from, to) = (
        cell(&viewer, cx, Column::Hex, 0),
        cell(&viewer, cx, Column::Hex, 3),
    );
    cx.simulate_click(from, Modifiers::none());
    cx.simulate_click(to, Modifiers::shift());
    cx.simulate_keystrokes(&copy);
    assert_eq!(copied(cx).as_deref(), Some("48 69 00 FF"));

    let (from, to) = (
        cell(&viewer, cx, Column::Ascii, 0),
        cell(&viewer, cx, Column::Ascii, 1),
    );
    cx.simulate_click(from, Modifiers::none());
    cx.simulate_click(to, Modifiers::shift());
    cx.simulate_keystrokes(&copy);
    assert_eq!(copied(cx).as_deref(), Some("Hi"));

    // In a header input the key copies nothing of the rows.
    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    cx.simulate_keystrokes("ctrl-g");
    draw(cx);
    cx.simulate_input("3");
    cx.simulate_keystrokes(&copy);
    assert_eq!(copied(cx).as_deref(), Some("before"));
}

#[gpui::test]
fn copy_as_hex_and_as_text(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(b"\xDE\xAD\xBE\xEFok\xC3\xA9\xFF!");
    let (viewer, cx, _) = viewer(cx, &path);
    let copied = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|item| item.text());

    viewer.update(cx, |viewer, _| viewer.select(0, 3));
    run(cx, COPY_AS_HEX);
    assert_eq!(copied(cx).as_deref(), Some("DE AD BE EF"));
    // From the ASCII column too: the command, not the column, says how.
    let at = cell(&viewer, cx, Column::Ascii, 4);
    cx.simulate_click(at, Modifiers::none());
    viewer.update(cx, |viewer, _| viewer.select(4, 9));
    run(cx, COPY_AS_HEX);
    assert_eq!(copied(cx).as_deref(), Some("6F 6B C3 A9 FF 21"));
    run(cx, COPY_AS_TEXT);
    assert_eq!(copied(cx).as_deref(), Some("oké\u{FFFD}!"));
    viewer.update(cx, |viewer, _| viewer.column = Column::Hex);
    run(cx, COPY_AS_TEXT);
    assert_eq!(copied(cx).as_deref(), Some("oké\u{FFFD}!"));
}

#[gpui::test]
fn copy_over_the_limit_is_refused(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("large.bin");
    let len = fe_editor::COPY_LIMIT as u64 + 16;
    std::fs::File::create(&path).unwrap().set_len(len).unwrap();
    let (viewer, cx, events) = viewer(cx, &path);
    let copy = format!("{}-c", secondary());
    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));

    cx.simulate_keystrokes(&format!("shift-{}-down", secondary()));
    assert_eq!(selected(&viewer, cx), 0..len);
    cx.simulate_keystrokes(&copy);
    run(cx, COPY_AS_TEXT);
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("before"));
    assert_eq!(
        *events.borrow(),
        [
            HexViewerEvent::CopyTooLarge { len },
            HexViewerEvent::CopyTooLarge { len },
        ]
    );
    // What's in the limit still copies.
    viewer.update(cx, |viewer, _| viewer.select(len - 2, len - 1));
    cx.simulate_keystrokes(&copy);
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("00 00"));
}

#[gpui::test]
fn a_narrow_pane_scrolls_sideways_and_keeps_its_buttons(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(1024));
    let (viewer, cx, _) = viewer(cx, &path);
    cx.simulate_resize(gpui::size(px(320.), px(400.)));
    draw(cx);
    let layout = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.layout.unwrap());
    let ascii_end = |cx: &mut VisualTestContext| {
        let layout = layout(cx);
        let cell = Columns::for_len(1024).cell(Column::Ascii, 15);
        layout.x(cell.end)
    };
    assert!(
        ascii_end(cx) > layout(cx).rows.right(),
        "a row is wider than the pane"
    );

    // The wheel scrolls sideways to the row's end, and no further.
    let at = layout(cx).rows.center();
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(-2000.), px(0.))),
        ..Default::default()
    });
    draw(cx);
    let (right, end) = (layout(cx).rows.right(), ascii_end(cx));
    assert!(end <= right && end > right - px(20.), "{end:?} {right:?}");
    // The last ASCII byte, clicked, is the one under the pointer.
    let last = cell(&viewer, cx, Column::Ascii, 15);
    cx.simulate_click(last, Modifiers::none());
    assert_eq!(selected(&viewer, cx), 15..16);

    // Keys bring the head's cell back into view.
    viewer.update(cx, |viewer, _| viewer.column = Column::Hex);
    cx.simulate_keystrokes("home");
    draw(cx);
    let first = cell(&viewer, cx, Column::Hex, 0);
    assert!(first.x >= layout(cx).rows.left(), "{first:?}");

    // A long query that doesn't parse leaves the Text button in the pane.
    cx.simulate_keystrokes(&format!("{}-f", secondary()));
    draw(cx);
    cx.simulate_input("not hex at all, and far too long for the header of a narrow pane");
    draw(cx);
    let button = cx
        .debug_bounds(verbatim!("hex-viewer-open-as-text"))
        .expect("drawn");
    let header = cx
        .debug_bounds(verbatim!("hex-viewer-header"))
        .expect("drawn");
    assert!(
        button.right() <= header.right() && button.left() >= header.left(),
        "{button:?} in {header:?}"
    );
}
