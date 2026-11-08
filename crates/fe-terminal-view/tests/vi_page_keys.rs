//! Vi copy mode's ctrl-f and ctrl-b: they page the view by two rows less
//! than its height and put the vi cursor on the new top or bottom row.

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

/// The vi cursor's row text, its screen row and column, and how far back the view is
/// scrolled.
fn at(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> (String, usize, usize, usize) {
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        assert!(snapshot.modes.vi, "not in vi mode");
        (
            snapshot.row_text(snapshot.cursor.row).trim_end().to_owned(),
            snapshot.cursor.row,
            snapshot.cursor.col,
            snapshot.display_offset,
        )
    })
}

#[gpui::test]
fn ctrl_f_and_ctrl_b_put_the_vi_cursor_on_the_new_top_or_bottom_row(cx: &mut TestAppContext) {
    let script = "i=1; while [ $i -le 120 ]; do echo line $i; i=$((i+1)); done; read x";
    let (view, cx) = open(cx, script, "line 120");
    let rows = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().rows.len());
    assert!(rows > 20 && rows < 40, "{rows} rows");
    let page = rows - 2;

    cx.simulate_keystrokes("ctrl-shift-space g g 4 j 3 l");
    let (text, row, col, top) = at(&view, cx);
    assert_eq!((text.as_str(), row, col), ("line 5", 4, 3));

    // The view pages, the cursor takes its top row and keeps its column.
    cx.simulate_keystrokes("ctrl-f");
    let want = format!("line {}", 1 + page);
    assert_eq!(at(&view, cx), (want, 0, 3, top - page));
    // And back, onto the bottom row, to the top of history.
    cx.simulate_keystrokes("ctrl-b");
    let want = format!("line {}", rows);
    assert_eq!(at(&view, cx), (want, rows - 1, 3, top));

    cx.simulate_keystrokes("2 ctrl-f");
    let want = format!("line {}", 1 + 2 * page);
    assert_eq!(at(&view, cx), (want, 0, 3, top - 2 * page));
    cx.simulate_keystrokes("2 ctrl-b");
    let want = format!("line {}", rows);
    assert_eq!(at(&view, cx), (want, rows - 1, 3, top));

    // Pagedown and pageup do the same.
    cx.simulate_keystrokes("pagedown");
    let want = format!("line {}", 1 + page);
    assert_eq!(at(&view, cx), (want, 0, 3, top - page));
    cx.simulate_keystrokes("pageup");
    let want = format!("line {}", rows);
    assert_eq!(at(&view, cx), (want, rows - 1, 3, top));

    // At the top of history ctrl-b moves nothing.
    cx.simulate_keystrokes("g g");
    let here = at(&view, cx);
    cx.simulate_keystrokes("ctrl-b");
    assert_eq!(at(&view, cx), here);

    // Within a page of the bottom ctrl-f scrolls to it, the cursor on the
    // top row; at the bottom it goes to the last row.
    cx.simulate_keystrokes("G");
    cx.simulate_keystrokes("ctrl-b");
    let (_, _, _, offset) = at(&view, cx);
    assert_eq!(offset, page);
    cx.simulate_keystrokes("ctrl-f");
    let (_, row, col, offset) = at(&view, cx);
    assert_eq!((row, offset), (0, 0));
    assert_eq!(col, 0);
    cx.simulate_keystrokes("ctrl-f");
    let (text, row, _, offset) = at(&view, cx);
    assert_eq!((text.as_str(), row, offset), ("", rows - 1, 0));
}
