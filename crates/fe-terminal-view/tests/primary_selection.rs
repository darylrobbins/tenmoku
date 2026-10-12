//! The Linux primary selection in a terminal view: a selection is written to
//! it, and a middle click pastes it, over a real pty.
#![cfg(any(target_os = "linux", target_os = "freebsd"))]

use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView};
use gpui::{
    ClipboardItem, Entity, Hsla, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    Point, Size, TestAppContext, VisualTestContext, point, px, rgb, size,
};

fn style() -> TerminalStyle {
    let background: Hsla = rgb(0x101010).into();
    TerminalStyle {
        font_family: "Menlo".into(),
        font_size: px(14.),
        line_height: TerminalStyle::LINE_HEIGHT,
        background,
        foreground: rgb(0xd0d0d0).into(),
        cursor: rgb(0xffcc00).into(),
        selection: rgb(0x334466).into(),
        ansi: TerminalStyle::ansi_for(background),
    }
}

/// `script`, run by `/bin/sh -c`.
fn options(script: &str) -> TerminalOptions {
    let mut options = TerminalOptions::new(std::env::temp_dir());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), script.to_owned()],
    ));
    options
}

/// Raw, unechoed input, each read printed as a line of hex bytes, after
/// `setup` is printed and `ready` said.
fn hex_reader(setup: &str) -> TerminalOptions {
    let hex = "dd bs=64 count=1 2>/dev/null | od -An -tx1 -v";
    options(&format!(
        "stty raw -echo opost; printf '{setup}'; echo ready; while :; do {hex}; done"
    ))
}

/// Draws until `done` holds. Panics after 5 s, naming `what`.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Draws for a while, so anything still on its way arrives.
fn settle(cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        thread::sleep(Duration::from_millis(10));
    }
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.terminal().map(|t| t.snapshot().text()))
        .unwrap_or_default()
}

fn row_of(view: &Entity<TerminalView>, cx: &mut VisualTestContext, line: &str) -> usize {
    let snapshot = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot());
    (0..snapshot.rows.len())
        .find(|&row| snapshot.row_text(row) == line)
        .unwrap_or_else(|| panic!("no {line:?} on the screen:\n{}", snapshot.text()))
}

fn wait_for_line(view: &Entity<TerminalView>, cx: &mut VisualTestContext, line: &str) {
    until(cx, line, |cx| screen(view, cx).lines().any(|l| l == line));
}

/// Every byte the program has printed in hex so far.
fn written(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Vec<u8> {
    screen(view, cx)
        .split_whitespace()
        .filter(|word| word.len() == 2)
        .filter_map(|word| u8::from_str_radix(word, 16).ok())
        .collect()
}

/// Waits until the program has read exactly `want` in all, and checks
/// nothing more follows.
fn expect_written(view: &Entity<TerminalView>, cx: &mut VisualTestContext, want: &[u8]) {
    until(cx, &format!("the program to read {want:02x?}"), |cx| {
        written(view, cx).len() >= want.len()
    });
    settle(cx);
    assert_eq!(written(view, cx), want, "the screen:\n{}", screen(view, cx));
}

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

fn at(cell: Size<Pixels>, row: usize, col: usize, x: f32) -> Point<Pixels> {
    point(
        cell.width * (col as f32 + x),
        cell.height * (row as f32 + 0.5),
    )
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    options: TerminalOptions,
    line: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    // The pty's threads wake the view's event task.
    cx.executor().allow_parking();
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    cx.simulate_resize(size(px(800.), px(600.)));
    wait_for_line(&view, cx, line);
    (view, cx)
}

fn press(
    cx: &mut VisualTestContext,
    button: MouseButton,
    position: Point<Pixels>,
    clicks: usize,
    modifiers: Modifiers,
) {
    cx.simulate_event(MouseDownEvent {
        position,
        button,
        modifiers,
        click_count: clicks,
        first_mouse: false,
    });
}

fn release(
    cx: &mut VisualTestContext,
    button: MouseButton,
    position: Point<Pixels>,
    clicks: usize,
    modifiers: Modifiers,
) {
    cx.simulate_event(MouseUpEvent {
        position,
        button,
        modifiers,
        click_count: clicks,
    });
}

fn click(cx: &mut VisualTestContext, position: Point<Pixels>, clicks: usize, modifiers: Modifiers) {
    press(cx, MouseButton::Left, position, clicks, modifiers);
    release(cx, MouseButton::Left, position, clicks, modifiers);
}

fn middle_click(cx: &mut VisualTestContext, position: Point<Pixels>, modifiers: Modifiers) {
    press(cx, MouseButton::Middle, position, 1, modifiers);
    release(cx, MouseButton::Middle, position, 1, modifiers);
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>, modifiers: Modifiers) {
    press(cx, MouseButton::Left, from, 1, modifiers);
    cx.simulate_mouse_move(to, MouseButton::Left, modifiers);
    release(cx, MouseButton::Left, to, 1, modifiers);
}

fn primary(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text()))
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn set_primary(cx: &mut VisualTestContext, text: &str) {
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string(text.to_owned())));
}

#[gpui::test]
fn selecting_writes_the_primary_selection(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        options("echo 'hello world'; exec sleep 60"),
        "hello world",
    );
    let grid = cell(cx);
    let row = row_of(&view, cx, "hello world");
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("clip".to_owned())));
    let none = Modifiers::none();

    drag(cx, at(grid, row, 0, 0.2), at(grid, row, 4, 0.8), none);
    assert_eq!(primary(cx).as_deref(), Some("hello"));

    click(cx, at(grid, row, 8, 0.5), 2, none);
    assert_eq!(primary(cx).as_deref(), Some("world"));

    // Shift-click extends, and writes the extended selection.
    drag(cx, at(grid, row, 0, 0.2), at(grid, row, 1, 0.8), none);
    assert_eq!(primary(cx).as_deref(), Some("he"));
    click(cx, at(grid, row, 7, 0.8), 1, Modifiers::shift());
    assert_eq!(primary(cx).as_deref(), Some("hello wo"));

    // The clipboard is left alone.
    assert_eq!(clipboard(cx).as_deref(), Some("clip"));

    // Off, nothing is written.
    view.update(cx, |view, _| view.set_primary_selection(false));
    drag(cx, at(grid, row, 0, 0.2), at(grid, row, 2, 0.8), none);
    assert_eq!(primary(cx).as_deref(), Some("hello wo"));
}

#[gpui::test]
fn a_drag_reported_to_a_program_writes_nothing(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        hex_reader("hello world\\n\\033[?1002h\\033[?1006h"),
        "ready",
    );
    let grid = cell(cx);
    let row = row_of(&view, cx, "hello world");
    // Frames are laid out in the modes last drawn: wait for the program's.
    until(cx, "mouse reporting", |cx| {
        view.read_with(cx, |view, _| {
            view.terminal().unwrap().snapshot().modes.mouse_drag
        })
    });
    settle(cx);
    set_primary(cx, "before");
    drag(
        cx,
        at(grid, row, 0, 0.2),
        at(grid, row, 4, 0.8),
        Modifiers::none(),
    );
    until(cx, "the reports", |cx| !written(&view, cx).is_empty());
    assert_eq!(primary(cx).as_deref(), Some("before"));
}

#[gpui::test]
fn middle_click_pastes_the_primary_selection(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader(""), "ready");
    let grid = cell(cx);
    set_primary(cx, "a\nb");
    // Line breaks are typed as returns.
    middle_click(cx, at(grid, 2, 3, 0.5), Modifiers::none());
    expect_written(&view, cx, b"a\rb");

    // Bracketed once the program asked for it.
    let (view, cx) = open(cx, hex_reader("\\033[?2004h"), "ready");
    until(cx, "bracketed paste", |cx| {
        view.read_with(cx, |view, _| {
            view.terminal().unwrap().snapshot().modes.bracketed_paste
        })
    });
    middle_click(cx, at(grid, 2, 3, 0.5), Modifiers::none());
    expect_written(&view, cx, b"\x1b[200~a\nb\x1b[201~");
}

#[gpui::test]
fn middle_click_with_reporting_on_is_reported(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1000h\\033[?1006h"), "ready");
    let grid = cell(cx);
    until(cx, "mouse reporting", |cx| {
        view.read_with(cx, |view, _| {
            view.terminal().unwrap().snapshot().modes.mouse_report_click
        })
    });
    settle(cx);
    set_primary(cx, "pasted");
    let position = at(grid, 2, 3, 0.5);
    middle_click(cx, position, Modifiers::none());
    // Middle is button 1, at column 4, row 3.
    expect_written(&view, cx, b"\x1b[<1;4;3M\x1b[<1;4;3m");

    // With shift held, it pastes.
    let before = written(&view, cx).len();
    press(cx, MouseButton::Middle, position, 1, Modifiers::shift());
    until(cx, "the paste", |cx| written(&view, cx).len() > before);
    settle(cx);
    assert_eq!(&written(&view, cx)[before..], b"pasted");
}
