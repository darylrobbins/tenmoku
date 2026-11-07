//! Vi copy mode's ctrl-e and ctrl-y: they scroll the view and leave the vi
//! cursor on its line, moving it only to keep it in view.

// Unix only: the emulator is fed `/bin/sh` scripts. It is the same code on
// Windows, where ConPTY re-renders the stream (see fe-terminal's conpty.rs).
#![cfg(unix)]

use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView};
use gpui::{Entity, Focusable as _, Hsla, TestAppContext, VisualTestContext, px, rgb, size};

fn style() -> TerminalStyle {
    let background: Hsla = rgb(0x101010).into();
    TerminalStyle {
        font_family: "Menlo".into(),
        font_fallbacks: Vec::new(),
        font_size: px(14.),
        line_height: TerminalStyle::LINE_HEIGHT,
        background,
        foreground: rgb(0xd0d0d0).into(),
        cursor: rgb(0xffcc00).into(),
        selection: rgb(0x334466).into(),
        ansi: TerminalStyle::ansi_for(background),
    }
}

/// The view's height: its bottom, where the search prompt is drawn.
const HEIGHT: f32 = 400.;

/// Draws until `done` holds. Panics after 5 s, naming `what`.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.terminal().map(|t| t.snapshot().text()))
        .unwrap_or_default()
}

/// A focused view running `/bin/sh -c script`, once its screen shows
/// `ready`.
fn open<'a>(
    cx: &'a mut TestAppContext,
    script: &str,
    ready: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    // The pty's threads wake the view's event task.
    cx.executor().allow_parking();
    let mut options = TerminalOptions::new(std::env::temp_dir());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), script.to_owned()],
    ));
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    cx.simulate_resize(size(px(600.), px(HEIGHT)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx)
    });
    until(cx, ready, |cx| screen(&view, cx).contains(ready));
    (view, cx)
}

/// The vi cursor's row text, its screen row, and how far back the view is
/// scrolled.
fn at(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> (String, usize, usize) {
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        assert!(snapshot.modes.vi, "not in vi mode");
        (
            snapshot.row_text(snapshot.cursor.row).trim_end().to_owned(),
            snapshot.cursor.row,
            snapshot.display_offset,
        )
    })
}

#[gpui::test]
fn ctrl_e_and_ctrl_y_keep_the_vi_cursor_on_its_line(cx: &mut TestAppContext) {
    let script = "i=1; while [ $i -le 60 ]; do echo line $i; i=$((i+1)); done; read x";
    let (view, cx) = open(cx, script, "line 60");
    let rows = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().rows.len());
    assert!(rows > 20 && rows < 40, "{rows} rows");

    cx.simulate_keystrokes("ctrl-shift-space g g 1 9 j");
    let (text, row, offset) = at(&view, cx);
    assert_eq!(text, "line 20");
    assert!(row > 3 && offset > row + 3, "row {row}, offset {offset}");

    // The view scrolls a line, the cursor stays on its text.
    cx.simulate_keystrokes("ctrl-e");
    assert_eq!(at(&view, cx), ("line 20".to_owned(), row - 1, offset - 1));
    cx.simulate_keystrokes("ctrl-y");
    assert_eq!(at(&view, cx), ("line 20".to_owned(), row, offset));
    cx.simulate_keystrokes("2 ctrl-e");
    assert_eq!(at(&view, cx), ("line 20".to_owned(), row - 2, offset - 2));
    cx.simulate_keystrokes("2 ctrl-y");
    assert_eq!(at(&view, cx), ("line 20".to_owned(), row, offset));

    // Scrolled until its line would leave the view, the cursor holds the
    // top row.
    for _ in 0..row + 3 {
        cx.simulate_keystrokes("ctrl-e");
    }
    let (text, top, _) = at(&view, cx);
    assert_eq!((text.as_str(), top), ("line 23", 0));
    // At the bottom the view can't scroll, and the cursor stays put.
    cx.simulate_keystrokes("G");
    let bottom = at(&view, cx);
    assert_eq!((bottom.1, bottom.2), (rows - 1, 0));
    cx.simulate_keystrokes("ctrl-e");
    assert_eq!(at(&view, cx), bottom);

    // And at the top of history ctrl-y leaves it too.
    cx.simulate_keystrokes("g g");
    let (text, row, offset) = at(&view, cx);
    assert_eq!(row, 0);
    cx.simulate_keystrokes("ctrl-y");
    assert_eq!(at(&view, cx), (text, row, offset));
}
