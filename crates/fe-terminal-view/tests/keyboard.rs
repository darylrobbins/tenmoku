//! Typing into a terminal view: the bytes keys, composed text and pastes
//! write to the pty, seen by a program that prints each read in hex.

use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView};
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, EntityInputHandler as _, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement, Keystroke, ParentElement as _, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, px, rgb, size,
};

/// The platform's copy and paste keys.
const COPY: &str = if cfg!(target_os = "macos") {
    "cmd-c"
} else {
    "ctrl-shift-c"
};
const PASTE: &str = if cfg!(target_os = "macos") {
    "cmd-v"
} else {
    "ctrl-shift-v"
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

/// Raw, unechoed input, each read printed as a line of hex bytes. After the
/// first read the program turns on application cursor keys and bracketed
/// paste, and says `switched`.
fn options() -> TerminalOptions {
    let hex = "dd bs=64 count=1 2>/dev/null | od -An -tx1 -v";
    let script = format!(
        "stty raw -echo opost; echo ready; {hex}; printf '\\033[?1h\\033[?2004h'; \
         echo switched; while :; do {hex}; done"
    );
    let mut options = TerminalOptions::new(std::env::temp_dir());
    options.shell = Some(("/bin/sh".to_owned(), vec!["-c".to_owned(), script]));
    options
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

/// Every byte the program has printed so far.
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

fn wait_for_line(view: &Entity<TerminalView>, cx: &mut VisualTestContext, line: &str) {
    until(cx, line, |cx| {
        screen(view, cx).lines().any(|l| l.trim_end() == line)
    });
}

/// A focused terminal view running [`options`], with the program ready.
fn open(cx: &mut TestAppContext) -> (Entity<TerminalView>, &mut VisualTestContext) {
    // The pty's threads wake the view's event task.
    cx.executor().allow_parking();
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options(), style(), window, cx).unwrap());
    cx.simulate_resize(size(px(800.), px(800.)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    wait_for_line(&view, cx, "ready");
    (view, cx)
}

#[gpui::test]
fn typed_text_reaches_the_shell(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    cx.simulate_input("héllo");
    let mut want = "héllo".as_bytes().to_vec();
    expect_written(&view, cx, &want);

    // A composition stays on screen, not in the pty, until it commits.
    view.update_in(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "に", Some(1..1), window, cx);
        view.replace_and_mark_text_in_range(None, "にほ", Some(2..2), window, cx);
    });
    settle(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.marked_text().map(str::to_owned)),
        Some("にほ".to_owned())
    );
    let marked = view.update_in(cx, |view, window, cx| view.marked_text_range(window, cx));
    assert_eq!(marked, Some(0..2));
    let bounds = view.update_in(cx, |view, window, cx| {
        let element = gpui::Bounds::new(gpui::point(px(0.), px(0.)), size(px(800.), px(800.)));
        view.bounds_for_range(0..2, element, window, cx)
    });
    assert!(bounds.is_some(), "the input method has nowhere to sit");
    assert_eq!(written(&view, cx), want, "marked text reached the pty");

    view.update_in(cx, |view, window, cx| {
        view.replace_text_in_range(None, "日本", window, cx);
    });
    want.extend_from_slice("日本".as_bytes());
    expect_written(&view, cx, &want);
    assert_eq!(
        view.read_with(cx, |view, _| view.marked_text().map(str::to_owned)),
        None
    );
}

#[gpui::test]
fn special_keys(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    cx.simulate_keystrokes("up");
    let mut want = b"\x1b[A".to_vec();
    expect_written(&view, cx, &want);

    // The program turned on application cursor keys (ESC[?1h).
    wait_for_line(&view, cx, "switched");
    let app_cursor = view.read_with(cx, |view, _| {
        view.terminal().unwrap().snapshot().modes.app_cursor
    });
    assert!(app_cursor);
    for (keys, bytes) in [
        ("up", &b"\x1bOA"[..]),
        ("ctrl-c", b"\x03"),
        ("enter", b"\r"),
        ("tab", b"\t"),
        ("backspace", b"\x7f"),
    ] {
        cx.simulate_keystrokes(keys);
        want.extend_from_slice(bytes);
        expect_written(&view, cx, &want);
    }

    cx.simulate_keystrokes("alt-left");
    if cfg!(target_os = "macos") {
        want.extend_from_slice(b"\x1bb");
    } else {
        want.extend_from_slice(b"\x1b[1;3D");
    }
    expect_written(&view, cx, &want);
}

/// Hosts a terminal view, recording the key downs that reach it.
struct Parent {
    terminal: Entity<TerminalView>,
    keys: Rc<RefCell<Vec<Keystroke>>>,
}

impl Render for Parent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let keys = self.keys.clone();
        div()
            .size_full()
            .on_key_down(move |event, _, _| keys.borrow_mut().push(event.keystroke.clone()))
            .child(self.terminal.clone())
    }
}

#[gpui::test]
fn secondary_keys_propagate(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let keys = Rc::new(RefCell::new(Vec::new()));
    let (parent, cx) = cx.add_window_view(|window, cx| Parent {
        terminal: cx.new(|cx| TerminalView::new(options(), style(), window, cx).unwrap()),
        keys: keys.clone(),
    });
    let view = parent.read_with(cx, |parent, _| parent.terminal.clone());
    cx.simulate_resize(size(px(800.), px(800.)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    wait_for_line(&view, cx, "ready");

    let cmd_p = Keystroke::parse("cmd-p").unwrap();
    cx.simulate_keystrokes("cmd-p");
    assert_eq!(*keys.borrow(), std::slice::from_ref(&cmd_p));
    // The terminal's own keys stop at it.
    cx.simulate_keystrokes("ctrl-c");
    assert_eq!(*keys.borrow(), [cmd_p]);
    // Only ctrl-c reached the pty.
    expect_written(&view, cx, b"\x03");
}

#[gpui::test]
fn copy_and_paste(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let select_ready = |view: &TerminalView| {
        let terminal = view.terminal().unwrap();
        let snapshot = terminal.snapshot();
        let row = (0..snapshot.rows.len())
            .find(|&row| snapshot.row_text(row).trim_end() == "ready")
            .unwrap();
        terminal.select(
            fe_terminal::SelectionKind::Simple,
            row,
            0,
            fe_terminal::Side::Left,
        );
        terminal.extend_selection(row, 4, fe_terminal::Side::Right);
    };
    view.read_with(cx, |view, _| select_ready(view));
    cx.simulate_keystrokes(COPY);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("ready")
    );

    // With nothing selected, copy leaves the clipboard and the pty alone.
    view.read_with(cx, |view, _| view.terminal().unwrap().clear_selection());
    cx.write_to_clipboard(ClipboardItem::new_string("untouched".to_owned()));
    cx.simulate_keystrokes(COPY);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("untouched")
    );

    // Without bracketed paste, line breaks are typed as returns.
    cx.write_to_clipboard(ClipboardItem::new_string("a\nb".to_owned()));
    cx.simulate_keystrokes(PASTE);
    let mut want = b"a\rb".to_vec();
    expect_written(&view, cx, &want);

    // The program turned on bracketed paste (ESC[?2004h).
    wait_for_line(&view, cx, "switched");
    let bracketed = view.read_with(cx, |view, _| {
        view.terminal().unwrap().snapshot().modes.bracketed_paste
    });
    assert!(bracketed);
    cx.simulate_keystrokes(PASTE);
    want.extend_from_slice(b"\x1b[200~a\nb\x1b[201~");
    expect_written(&view, cx, &want);
}
