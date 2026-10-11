//! The mouse in a terminal view: what a report encodes, and selecting,
//! scrolling back and reporting in a window over a real pty.

use std::thread;
use std::time::{Duration, Instant};

use fe_terminal::Modes;
use fe_terminal::keys::Mods;
use fe_terminal_view::mouse::{self, Action, Button, Event};
use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView};
use gpui::{
    Entity, Hsla, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, Size, TestAppContext, VisualTestContext, point, px, rgb, size,
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

fn event(action: Action, row: usize, col: usize, mods: Mods) -> Event {
    Event {
        action,
        row,
        col,
        mods,
    }
}

#[test]
fn encode() {
    let off = Modes::default();
    let click = Modes {
        mouse_report_click: true,
        ..Modes::default()
    };
    let sgr = Modes {
        mouse_drag: true,
        sgr_mouse: true,
        ..Modes::default()
    };
    let press = event(Action::Press(Button::Left), 4, 9, Mods::NONE);
    let encode = |event, modes: &Modes| mouse::encode(event, modes).map(String::from_utf8);

    // Nothing while reporting is off, nor drags or moves the program didn't
    // ask for.
    assert_eq!(mouse::encode(press, &off), None);
    let drag = event(Action::Drag(Button::Left), 4, 9, Mods::NONE);
    assert_eq!(mouse::encode(drag, &click), None);
    let moved = event(Action::Move, 4, 9, Mods::NONE);
    assert_eq!(mouse::encode(moved, &sgr), None);

    // SGR: 1-based column then row, M for a press and m for a release.
    assert_eq!(encode(press, &sgr), Some(Ok("\x1b[<0;10;5M".to_owned())));
    let release = event(Action::Release(Button::Right), 0, 0, Mods::NONE);
    assert_eq!(encode(release, &sgr), Some(Ok("\x1b[<2;1;1m".to_owned())));
    // Shift 4, alt 8, ctrl 16; a drag adds 32.
    let drag = event(Action::Drag(Button::Middle), 1, 2, Mods::SHIFT);
    assert_eq!(encode(drag, &sgr), Some(Ok("\x1b[<37;3;2M".to_owned())));
    let all = Mods {
        shift: true,
        alt: true,
        ctrl: true,
    };
    let drag = event(Action::Drag(Button::Left), 1, 2, all);
    assert_eq!(encode(drag, &sgr), Some(Ok("\x1b[<60;3;2M".to_owned())));
    // The wheel is buttons 64 and 65, and is never released.
    let wheel = event(Action::Press(Button::WheelUp), 2, 3, Mods::CTRL);
    assert_eq!(encode(wheel, &sgr), Some(Ok("\x1b[<80;4;3M".to_owned())));
    let wheel = event(Action::Press(Button::WheelDown), 2, 3, Mods::ALT);
    assert_eq!(encode(wheel, &sgr), Some(Ok("\x1b[<73;4;3M".to_owned())));
    let wheel = event(Action::Release(Button::WheelUp), 2, 3, Mods::NONE);
    assert_eq!(mouse::encode(wheel, &sgr), None);
    // Motion with no button held is 35, with any-motion tracking on.
    let motion = Modes {
        mouse_motion: true,
        sgr_mouse: true,
        ..Modes::default()
    };
    assert_eq!(
        encode(moved, &motion),
        Some(Ok("\x1b[<35;10;5M".to_owned()))
    );

    // X10: each value a byte offset by 32, and a release is button 3.
    assert_eq!(
        mouse::encode(press, &click),
        Some(b"\x1b[M\x20\x2a\x25".to_vec())
    );
    let release = event(Action::Release(Button::Left), 4, 9, Mods::NONE);
    assert_eq!(
        mouse::encode(release, &click),
        Some(b"\x1b[M\x23\x2a\x25".to_vec())
    );
    let shifted = event(Action::Press(Button::Right), 0, 0, Mods::SHIFT);
    assert_eq!(
        mouse::encode(shifted, &click),
        Some(b"\x1b[M\x26\x21\x21".to_vec())
    );
    // 223 is the last coordinate a byte carries; past it, nothing is sent.
    let last = event(Action::Press(Button::Left), 222, 222, Mods::NONE);
    assert_eq!(
        mouse::encode(last, &click),
        Some(vec![0x1b, b'[', b'M', 32, 255, 255])
    );
    let past = event(Action::Press(Button::Left), 0, 223, Mods::NONE);
    assert_eq!(mouse::encode(past, &click), None);
    let past = event(Action::Press(Button::Left), 223, 0, Mods::NONE);
    assert_eq!(mouse::encode(past, &click), None);
    // SGR has no such limit.
    assert_eq!(encode(past, &sgr), Some(Ok("\x1b[<0;1;224M".to_owned())));
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

fn snapshot(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> fe_terminal::Snapshot {
    view.read_with(cx, |view, _| view.terminal().unwrap().snapshot())
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.terminal().map(|t| t.snapshot().text()))
        .unwrap_or_default()
}

fn selected(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.terminal().unwrap().selected_text())
}

/// The viewport row showing `line`.
fn row_of(view: &Entity<TerminalView>, cx: &mut VisualTestContext, line: &str) -> usize {
    let snapshot = snapshot(view, cx);
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

/// A cell's size, as the element works it out: the advance of `m` wide and
/// 1.25 × the font size, rounded, high.
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

/// A point in cell (`row`, `col`): `x` across it from its left edge, as a
/// fraction of its width, and half way down.
fn at(cell: Size<Pixels>, row: usize, col: usize, x: f32) -> Point<Pixels> {
    point(
        cell.width * (col as f32 + x),
        cell.height * (row as f32 + 0.5),
    )
}

/// A terminal view running `options`, focused, until `line` shows.
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

fn press(cx: &mut VisualTestContext, position: Point<Pixels>, clicks: usize, modifiers: Modifiers) {
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers,
        click_count: clicks,
        first_mouse: false,
    });
}

fn release(
    cx: &mut VisualTestContext,
    position: Point<Pixels>,
    clicks: usize,
    modifiers: Modifiers,
) {
    cx.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers,
        click_count: clicks,
    });
}

fn click(cx: &mut VisualTestContext, position: Point<Pixels>, clicks: usize, modifiers: Modifiers) {
    press(cx, position, clicks, modifiers);
    release(cx, position, clicks, modifiers);
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>, modifiers: Modifiers) {
    press(cx, from, 1, modifiers);
    cx.simulate_mouse_move(to, MouseButton::Left, modifiers);
    release(cx, to, 1, modifiers);
}

#[gpui::test]
fn drag_selects(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        options("echo 'hello world'; exec sleep 60"),
        "hello world",
    );
    let grid = cell(cx);
    let row = row_of(&view, cx, "hello world");

    // From the left half of `h` to the right half of `d`.
    let (from, to) = (at(grid, row, 0, 0.2), at(grid, row, 10, 0.8));
    drag(cx, from, to, Modifiers::none());
    assert_eq!(selected(&view, cx).as_deref(), Some("hello world"));
    settle(cx);
    let spans = snapshot(&view, cx).selection;
    assert_eq!(spans.len(), 1);
    assert_eq!(
        (spans[0].row, spans[0].start_col, spans[0].end_col),
        (row, 0, 11)
    );

    // A click clears it.
    click(cx, at(grid, row, 3, 0.2), 1, Modifiers::none());
    assert_eq!(selected(&view, cx), None);

    // A double click selects the word, a triple click the line.
    click(cx, at(grid, row, 8, 0.5), 2, Modifiers::none());
    assert_eq!(selected(&view, cx).as_deref(), Some("world"));
    click(cx, at(grid, row, 8, 0.5), 3, Modifiers::none());
    assert_eq!(
        selected(&view, cx)
            .map(|text| text.trim_end().to_owned())
            .as_deref(),
        Some("hello world")
    );

    // Shift-click extends the selection.
    let (from, to) = (at(grid, row, 0, 0.2), at(grid, row, 4, 0.8));
    drag(cx, from, to, Modifiers::none());
    assert_eq!(selected(&view, cx).as_deref(), Some("hello"));
    click(cx, at(grid, row, 7, 0.8), 1, Modifiers::shift());
    assert_eq!(selected(&view, cx).as_deref(), Some("hello wo"));
}

#[gpui::test]
fn wheel(cx: &mut TestAppContext) {
    // 500 lines, then, after a byte of input, the alternate screen with
    // alternate scroll on, reading input.
    let hex = "dd bs=64 count=1 2>/dev/null | od -An -tx1 -v";
    let script = format!(
        "stty raw -echo opost; seq 1 500; dd bs=1 count=1 >/dev/null 2>&1; \
         printf '\\033[?1049h\\033[?1007h'; echo ready; while :; do {hex}; done"
    );
    let (view, cx) = open(cx, options(&script), "500");
    let grid = cell(cx);
    let middle = at(grid, 5, 5, 0.5);
    let notch = |lines: f32, cx: &mut VisualTestContext| {
        cx.simulate_event(ScrollWheelEvent {
            position: middle,
            delta: ScrollDelta::Lines(point(0., lines)),
            ..Default::default()
        });
    };

    // Up the wheel is back into history, three lines a notch.
    let last = row_of(&view, cx, "500");
    notch(1., cx);
    let scrolled = snapshot(&view, cx);
    assert_eq!(scrolled.display_offset, 3);
    assert!(scrolled.history_size > 3);
    assert_eq!(scrolled.row_text(last), "497");
    notch(-1., cx);
    assert_eq!(snapshot(&view, cx).display_offset, 0);

    // Pixels scroll whole lines once they add up to one.
    let height = grid.height;
    for _ in 0..3 {
        cx.simulate_event(ScrollWheelEvent {
            position: middle,
            delta: ScrollDelta::Pixels(point(px(0.), height * 0.5)),
            ..Default::default()
        });
    }
    assert_eq!(snapshot(&view, cx).display_offset, 1);

    // Input scrolls to the bottom, and the program goes to the alternate
    // screen.
    view.read_with(cx, |view, _| view.terminal().unwrap().input(&b"x"[..]));
    wait_for_line(&view, cx, "ready");
    // The mouse goes by the modes of the frame last drawn.
    settle(cx);
    let modes = snapshot(&view, cx).modes;
    assert!(modes.alt_screen && modes.alternate_scroll);

    // There, a notch up is three up arrows, and the view doesn't scroll.
    notch(1., cx);
    expect_written(&view, cx, &b"\x1b[A".repeat(3));
    assert_eq!(snapshot(&view, cx).display_offset, 0);
}

#[gpui::test]
fn reporting(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1000h\\033[?1006h"), "ready");
    let grid = cell(cx);
    // The mouse goes by the modes of the frame last drawn.
    settle(cx);
    let modes = snapshot(&view, cx).modes;
    assert!(modes.mouse_report_click && modes.sgr_mouse);

    // A click at row 3, column 6 is reported 1-based, column first.
    let position = at(grid, 2, 5, 0.5);
    click(cx, position, 1, Modifiers::none());
    expect_written(&view, cx, b"\x1b[<0;6;3M\x1b[<0;6;3m");

    // With shift held the user selects, and the program hears nothing.
    let row = row_of(&view, cx, "ready");
    let (from, to) = (at(grid, row, 0, 0.2), at(grid, row, 4, 0.8));
    drag(cx, from, to, Modifiers::shift());
    assert_eq!(selected(&view, cx).as_deref(), Some("ready"));
    expect_written(&view, cx, b"\x1b[<0;6;3M\x1b[<0;6;3m");
}

fn button_down(cx: &mut VisualTestContext, position: Point<Pixels>, button: MouseButton) {
    cx.simulate_event(MouseDownEvent {
        position,
        button,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
}

fn button_up(cx: &mut VisualTestContext, position: Point<Pixels>, button: MouseButton) {
    cx.simulate_event(MouseUpEvent {
        position,
        button,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
}

fn wheel_lines(cx: &mut VisualTestContext, position: Point<Pixels>, lines: f32, shift: bool) {
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0., lines)),
        modifiers: if shift {
            Modifiers::shift()
        } else {
            Modifiers::none()
        },
        ..Default::default()
    });
}

#[gpui::test]
fn reports_keep_selection_and_scrollback(cx: &mut TestAppContext) {
    // Any-motion reporting, SGR, with history to scroll into.
    let hex = "dd bs=64 count=1 2>/dev/null | od -An -tx1 -v";
    let script = format!(
        "stty raw -echo opost; seq 1 200; printf '\\033[?1003h\\033[?1006h'; echo ready; \
         while :; do {hex}; done"
    );
    let (view, cx) = open(cx, options(&script), "ready");
    let grid = cell(cx);
    settle(cx);
    let row = row_of(&view, cx, "ready");

    // Shift selects and scrolls back past the program.
    drag(
        cx,
        at(grid, row, 0, 0.2),
        at(grid, row, 4, 0.8),
        Modifiers::shift(),
    );
    assert_eq!(selected(&view, cx).as_deref(), Some("ready"));
    wheel_lines(cx, at(grid, 5, 5, 0.5), 1., true);
    let offset = snapshot(&view, cx).display_offset;
    assert!(offset > 0);
    // The selection scrolled with the text; it's still there after.
    assert!(selected(&view, cx).is_some());

    // An ordinary movement reports to the program, and disturbs neither.
    cx.simulate_mouse_move(at(grid, 2, 2, 0.5), None, Modifiers::none());
    settle(cx);
    assert!(selected(&view, cx).is_some());
    // The program's reply scrolls history, which holds a held-back view in
    // place; it isn't sent to the bottom.
    assert!(snapshot(&view, cx).display_offset >= offset);
    // The program's reading is off screen; look at it.
    view.read_with(cx, |view, _| view.terminal().unwrap().scroll_to_bottom());
    expect_written(&view, cx, b"\x1b[<35;3;3M");
}

#[gpui::test]
fn overlapping_buttons_are_each_released(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1000h\\033[?1006h"), "ready");
    let grid = cell(cx);
    settle(cx);
    let position = at(grid, 2, 5, 0.5);
    button_down(cx, position, MouseButton::Left);
    button_down(cx, position, MouseButton::Right);
    button_up(cx, position, MouseButton::Left);
    button_up(cx, position, MouseButton::Right);
    expect_written(
        &view,
        cx,
        b"\x1b[<0;6;3M\x1b[<2;6;3M\x1b[<0;6;3m\x1b[<2;6;3m",
    );
}

#[gpui::test]
fn missed_release_is_reported_on_movement(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1002h\\033[?1006h"), "ready");
    let grid = cell(cx);
    settle(cx);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Left);
    // Movement with no button held: the release was lost.
    cx.simulate_mouse_move(at(grid, 4, 7, 0.5), None, Modifiers::none());
    expect_written(&view, cx, b"\x1b[<0;6;3M\x1b[<0;8;5m");
}

#[gpui::test]
fn shift_mid_drag_releases_to_the_program(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1002h\\033[?1006h"), "ready");
    let grid = cell(cx);
    settle(cx);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Left);
    cx.simulate_mouse_move(at(grid, 2, 6, 0.5), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(at(grid, 2, 7, 0.5), MouseButton::Left, Modifiers::shift());
    cx.simulate_mouse_move(at(grid, 2, 8, 0.5), MouseButton::Left, Modifiers::shift());
    button_up(cx, at(grid, 2, 8, 0.5), MouseButton::Left);
    expect_written(&view, cx, b"\x1b[<0;6;3M\x1b[<32;7;3M\x1b[<0;8;3m");
}

#[gpui::test]
fn shift_wheel_with_alternate_scroll_and_reporting_writes_nothing(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        hex_reader("\\033[?1049h\\033[?1007h\\033[?1000h\\033[?1006h"),
        "ready",
    );
    let grid = cell(cx);
    settle(cx);
    let modes = snapshot(&view, cx).modes;
    assert!(modes.alt_screen && modes.alternate_scroll && modes.mouse_report_click);
    let middle = at(grid, 5, 5, 0.5);
    wheel_lines(cx, middle, 1., true);
    settle(cx);
    assert!(written(&view, cx).is_empty());
    // Without shift the program hears the wheel.
    wheel_lines(cx, middle, 1., false);
    expect_written(&view, cx, b"\x1b[<64;6;6M");
}

#[gpui::test]
fn wheel_remainder_does_not_cross_destinations(cx: &mut TestAppContext) {
    let hex = "dd bs=64 count=1 2>/dev/null | od -An -tx1 -v";
    let script = format!(
        "stty raw -echo opost; seq 1 200; printf '\\033[?1000h\\033[?1006h'; echo ready; \
         while :; do {hex}; done"
    );
    let (view, cx) = open(cx, options(&script), "ready");
    let grid = cell(cx);
    settle(cx);
    let middle = at(grid, 5, 5, 0.5);
    // 2.5 lines toward a report, then half a line with shift: nothing
    // scrolls, since the half line alone isn't a whole one.
    wheel_lines(cx, middle, 2.5 / 3., false);
    wheel_lines(cx, middle, 0.5 / 3., true);
    assert_eq!(snapshot(&view, cx).display_offset, 0);
    settle(cx);
    assert!(written(&view, cx).is_empty());
}

/// A sibling drawn over the terminal's window that swallows mouse events.
struct Swallow {
    terminal: Entity<TerminalView>,
}

impl gpui::Render for Swallow {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{InteractiveElement, ParentElement, Styled};
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                gpui::div()
                    .w_full()
                    .h(px(300.))
                    .child(self.terminal.clone()),
            )
            .child(
                gpui::div()
                    .w_full()
                    .h(px(300.))
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_move(|_, _, cx| cx.stop_propagation()),
            )
    }
}

#[gpui::test]
fn gesture_ends_even_when_a_sibling_stops_propagation(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let options = hex_reader("\\033[?1002h\\033[?1006h");
    let terminal =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    let (terminal, _) = terminal;
    let (_, cx) = cx.add_window_view(|_, _| Swallow {
        terminal: terminal.clone(),
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    wait_for_line(&terminal, cx, "ready");
    settle(cx);
    let grid = cell(cx);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Left);
    // Release over the sibling, below the terminal.
    let below = point(px(100.), px(450.));
    cx.simulate_mouse_move(below, MouseButton::Left, Modifiers::none());
    button_up(cx, below, MouseButton::Left);
    let bytes = {
        settle(cx);
        written(&terminal, cx)
    };
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("\x1b[<0;6;3M"), "{text:?}");
    assert!(text.ends_with('m'), "{text:?}");
}

fn up_with(cx: &mut VisualTestContext, position: Point<Pixels>, modifiers: Modifiers) {
    cx.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers,
        click_count: 1,
    });
}

#[gpui::test]
fn movement_keeps_other_held_buttons(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1002h\\033[?1006h"), "ready");
    let grid = cell(cx);
    settle(cx);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Left);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Right);
    // Movement names only the right button; the left is still down.
    cx.simulate_mouse_move(at(grid, 2, 6, 0.5), MouseButton::Right, Modifiers::none());
    button_up(cx, at(grid, 2, 6, 0.5), MouseButton::Left);
    cx.simulate_mouse_move(at(grid, 2, 7, 0.5), MouseButton::Right, Modifiers::none());
    button_up(cx, at(grid, 2, 7, 0.5), MouseButton::Right);
    expect_written(
        &view,
        cx,
        b"\x1b[<0;6;3M\x1b[<2;6;3M\x1b[<34;7;3M\x1b[<0;7;3m\x1b[<34;8;3M\x1b[<2;8;3m",
    );
}

#[gpui::test]
fn shift_before_release_cancels_unshifted(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1002h\\033[?1006h"), "ready");
    let grid = cell(cx);
    settle(cx);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Left);
    up_with(cx, at(grid, 2, 5, 0.5), Modifiers::shift());
    expect_written(&view, cx, b"\x1b[<0;6;3M\x1b[<0;6;3m");
}

#[gpui::test]
fn shift_during_missed_release_recovery_is_unshifted(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, hex_reader("\\033[?1002h\\033[?1006h"), "ready");
    let grid = cell(cx);
    settle(cx);
    button_down(cx, at(grid, 2, 5, 0.5), MouseButton::Left);
    cx.simulate_mouse_move(at(grid, 4, 7, 0.5), None, Modifiers::shift());
    expect_written(&view, cx, b"\x1b[<0;6;3M\x1b[<0;8;5m");
}

#[gpui::test]
fn selection_autoscroll_stops_after_release_over_a_sibling(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let options = options("seq 1 200; echo ready; sleep 30");
    let (terminal, _) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    let (_, cx) = cx.add_window_view(|_, _| Swallow {
        terminal: terminal.clone(),
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    wait_for_line(&terminal, cx, "ready");
    settle(cx);
    let grid = cell(cx);
    // Well back into history, so there's room to scroll toward the bottom.
    terminal.read_with(cx, |view, _| view.terminal().unwrap().scroll(-10));
    let start = snapshot(&terminal, cx).display_offset;
    assert!(start > 3);
    press(cx, at(grid, 2, 5, 0.5), 1, Modifiers::none());
    let below = point(px(100.), px(450.));
    cx.simulate_mouse_move(below, MouseButton::Left, Modifiers::none());
    until(cx, "autoscroll", |cx| {
        snapshot(&terminal, cx).display_offset < start
    });
    release(cx, below, 1, Modifiers::none());
    settle(cx);
    let stopped = snapshot(&terminal, cx).display_offset;
    settle(cx);
    assert_eq!(snapshot(&terminal, cx).display_offset, stopped);
}
