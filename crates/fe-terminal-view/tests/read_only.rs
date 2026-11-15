//! A read-only terminal view (`set_read_only`): no key, paste or composed
//! text reaches the pty, while selection, copy, scroll, vi copy mode and
//! find still work.

// Unix only: the emulator is fed `/bin/sh` scripts. It is the same code on
// Windows, where ConPTY re-renders the stream (see fe-terminal's conpty.rs).
#![cfg(unix)]

use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalQuery, TerminalStyle, TerminalView};
use gpui::{
    ClipboardItem, Entity, Focusable as _, Hsla, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, Size, TestAppContext,
    VisualTestContext, point, px, rgb, size,
};

/// The platform's paste key.
const PASTE: &str = if cfg!(target_os = "macos") {
    "cmd-v"
} else {
    "ctrl-shift-v"
};

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

/// Raw, unechoed input, each read printed as a line of hex bytes.
const HEX: &str = "stty raw -echo opost; echo ready; \
     while :; do dd bs=64 count=1 2>/dev/null | od -An -tx1 -v; done";

/// Three marked prompts (OSC 133;A), each followed by 30 lines, then a
/// marked prompt waiting for input.
const PROMPTS: &str = r"for i in 1 2 3; do
        printf '\033]133;A\007$ prompt %d\n' $i
        j=1; while [ $j -le 30 ]; do echo out $i $j; j=$((j+1)); done
    done; printf '\033]133;A\007$ '; read x";

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

/// Draws for a while, so anything still on its way arrives.
fn settle(cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        draw(cx);
        thread::sleep(Duration::from_millis(10));
    }
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

fn written(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Vec<u8> {
    view.update(cx, |view, cx| {
        let was = view.is_vi_mode();
        if was {
            view.toggle_vi_mode(cx);
        }
        let text = view.terminal().unwrap().snapshot().text();
        if was {
            view.toggle_vi_mode(cx);
        }
        text
    })
    .split_whitespace()
    .filter(|word| word.len() == 2)
    .filter_map(|word| u8::from_str_radix(word, 16).ok())
    .collect()
}

#[gpui::test]
fn a_read_only_view_drops_input(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, HEX, "ready");
    view.update(cx, |view, cx| view.set_read_only(true, cx));
    assert!(view.read_with(cx, |view, _| view.is_read_only()));

    cx.simulate_keystrokes("a enter tab backspace ctrl-c ctrl-a up escape");
    cx.simulate_input("héllo");
    cx.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
    cx.simulate_keystrokes(PASTE);
    view.update_in(cx, |view, window, cx| {
        use gpui::EntityInputHandler as _;
        view.replace_and_mark_text_in_range(None, "に", None, window, cx);
        view.replace_text_in_range(None, "に", window, cx);
        view.paste(ClipboardItem::new_string("more".into()), window, cx);
    });
    assert_eq!(
        view.read_with(cx, |view, _| view.marked_text().map(str::to_owned)),
        None
    );
    settle(cx);
    assert_eq!(written(&view, cx), b"");

    // Writable again: keys reach it.
    view.update(cx, |view, cx| view.set_read_only(false, cx));
    cx.simulate_keystrokes("a");
    until(cx, "the program to read a", |cx| {
        !written(&view, cx).is_empty()
    });
    settle(cx);
    assert_eq!(written(&view, cx), b"a");
}

#[gpui::test]
fn a_read_only_view_still_selects_copies_scrolls_and_finds(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, PROMPTS, "out 3 30");
    view.update(cx, |view, cx| view.set_read_only(true, cx));

    // Vi copy mode: toggles, moves, scrolls, selects and yanks.
    cx.simulate_keystrokes("ctrl-shift-space");
    assert!(view.read_with(cx, |view, _| view.is_vi_mode()));
    let offset = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.terminal().unwrap().snapshot().display_offset
        })
    };
    let before = offset(cx);
    cx.simulate_keystrokes("ctrl-b");
    assert!(offset(cx) > before, "scrolled back");
    cx.simulate_keystrokes("g g 0 v 4 l y");
    assert!(!view.read_with(cx, |view, _| view.is_vi_mode()), "y leaves");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("$ pro"));

    // The copy key copies a selection too, and a paste does nothing, not
    // even leaving vi mode.
    cx.simulate_keystrokes("ctrl-shift-space 0 v 2 l");
    cx.write_to_clipboard(ClipboardItem::new_string("keep".into()));
    cx.simulate_keystrokes(PASTE);
    assert!(view.read_with(cx, |view, _| view.is_vi_mode()));
    cx.simulate_keystrokes("escape escape");

    // Find.
    view.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::literal("out 2 1")), cx)
            .unwrap();
    });
    let found = view.update(cx, |view, cx| view.find_next(true, cx).unwrap());
    assert!(found.is_some(), "find steps to a match");
    settle(cx);
}

/// A cell's size, as the element works it out.
fn cell(cx: &mut VisualTestContext) -> Size<Pixels> {
    let style = style();
    let width = cx.update(|window, _| {
        let text_system = window.text_system();
        let font = text_system.resolve_font(&gpui::font(style.font_family.clone()));
        text_system
            .advance(font, style.font_size, 'm')
            .unwrap()
            .width
    });
    size(width, (style.font_size * style.line_height).round())
}

/// A point in cell (`row`, `col`), `x` across it, half way down.
fn at(cell: Size<Pixels>, row: usize, col: usize, x: f32) -> Point<Pixels> {
    point(
        cell.width * (col as f32 + x),
        cell.height * (row as f32 + 0.5),
    )
}

fn wheel(cx: &mut VisualTestContext, position: Point<Pixels>, lines: f32) {
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0., lines)),
        ..Default::default()
    });
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_event(MouseDownEvent {
        position: from,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_event(MouseUpEvent {
        position: to,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
}

/// A hex reader with 200 lines of history, which has asked for mouse
/// reports and, once it reads a byte, the alternate screen with alternate
/// scroll.
const MOUSE_HUNGRY: &str = "stty raw -echo opost; i=1; while [ $i -le 200 ]; do echo L$i; i=$((i+1)); done; \
     printf '\\033[?1000h\\033[?1006h\\033[?1002h'; echo ready; \
     dd bs=1 count=1 >/dev/null 2>&1; printf '\\033[?1049h\\033[?1007h'; echo alt; \
     while :; do dd bs=64 count=1 2>/dev/null | od -An -tx1 -v; done";

#[gpui::test]
fn a_read_only_view_selects_and_scrolls_with_the_mouse_and_reports_nothing(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, MOUSE_HUNGRY, "ready");
    settle(cx);
    let modes = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().modes);
    assert!(modes.mouse_drag && modes.sgr_mouse, "{modes:?}");
    view.update(cx, |view, cx| view.set_read_only(true, cx));
    settle(cx);
    let grid = cell(cx);
    let middle = at(grid, 5, 5, 0.5);

    // The wheel scrolls the scrollback in place of reporting.
    wheel(cx, middle, 1.);
    let offset = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.terminal().unwrap().snapshot().display_offset
        })
    };
    assert_eq!(offset(cx), 3);
    wheel(cx, middle, -1.);
    assert_eq!(offset(cx), 0);

    // A drag selects, though the program asked for drags.
    let row = view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        (0..snapshot.rows.len())
            .find(|&row| snapshot.row_text(row) == "ready")
            .unwrap()
    });
    drag(cx, at(grid, row, 0, 0.2), at(grid, row, 4, 0.8));
    let selected = view.read_with(cx, |view, _| view.terminal().unwrap().selected_text());
    assert_eq!(selected.as_deref(), Some("ready"));

    // The program has heard nothing: it still waits for its first byte.
    settle(cx);
    let screen = screen(&view, cx);
    assert!(!screen.contains("alt"), "{screen}");
    view.update(cx, |view, cx| view.set_read_only(false, cx));
    view.read_with(cx, |view, _| view.terminal().unwrap().input(&b"x"[..]));
    until(cx, "the alternate screen", |cx| {
        screen_has(&view, cx, "alt")
    });
}

#[gpui::test]
fn a_read_only_view_sends_no_arrow_keys_for_the_wheel_on_the_alternate_screen(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, MOUSE_HUNGRY, "ready");
    view.read_with(cx, |view, _| view.terminal().unwrap().input(&b"x"[..]));
    until(cx, "the alternate screen", |cx| {
        screen_has(&view, cx, "alt")
    });
    settle(cx);
    let modes = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().modes);
    assert!(modes.alt_screen && modes.alternate_scroll, "{modes:?}");
    let middle = at(cell(cx), 5, 5, 0.5);

    // Writable, a notch is three arrow keys (and, with reporting on, mouse
    // presses): the program reads bytes.
    wheel(cx, middle, 1.);
    until(cx, "the program to hear the wheel", |cx| {
        !written(&view, cx).is_empty()
    });

    // Read-only, nothing more reaches it, however much it is wheeled.
    view.update(cx, |view, cx| view.set_read_only(true, cx));
    settle(cx);
    let before = written(&view, cx);
    for _ in 0..3 {
        wheel(cx, middle, 1.);
        wheel(cx, middle, -1.);
    }
    let grid = cell(cx);
    drag(cx, at(grid, 2, 0, 0.2), at(grid, 2, 3, 0.8));
    settle(cx);
    assert_eq!(written(&view, cx), before);
}

fn screen_has(view: &Entity<TerminalView>, cx: &mut VisualTestContext, text: &str) -> bool {
    screen(view, cx).contains(text)
}
