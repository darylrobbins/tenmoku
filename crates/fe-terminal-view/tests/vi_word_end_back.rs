//! `ge` and `gE` in a terminal's vi copy mode: back to the end of the
//! previous word or WORD, counted.

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

/// The view's height.
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

/// The text the vi cursor is on and its column.
fn on(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> (String, usize) {
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        assert!(snapshot.modes.vi, "not in vi mode");
        (snapshot.row_text(snapshot.cursor.row), snapshot.cursor.col)
    })
}

#[gpui::test]
fn ge_and_big_ge_move_back_to_the_previous_word_end(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "printf 'one two three four\\nab cd(ef) gh\\nready'; sleep 60",
        "ready",
    );
    cx.simulate_keystrokes("ctrl-shift-space");
    let four = ("one two three four".to_owned(), 12);
    let ab = |col| ("ab cd(ef) gh".to_owned(), col);

    cx.simulate_keystrokes("k k $ g e");
    assert_eq!(on(&view, cx), four);
    cx.simulate_keystrokes("g e");
    assert_eq!(on(&view, cx), ("one two three four".to_owned(), 6));
    cx.simulate_keystrokes("$ 2 g e");
    assert_eq!(on(&view, cx), ("one two three four".to_owned(), 6));
    cx.simulate_keystrokes("$ g shift-e");
    assert_eq!(on(&view, cx), four);
    cx.simulate_keystrokes("$ 3 g shift-e");
    assert_eq!(on(&view, cx), ("one two three four".to_owned(), 2));
    cx.simulate_keystrokes("j $ g shift-e");
    assert_eq!(on(&view, cx), ab(8));
    cx.simulate_keystrokes("g shift-e");
    assert_eq!(on(&view, cx), ab(1));
    cx.simulate_keystrokes("$ g e");
    assert_eq!(on(&view, cx), ab(8));
    cx.simulate_keystrokes("g e");
    assert_eq!(on(&view, cx), ab(7));
}
