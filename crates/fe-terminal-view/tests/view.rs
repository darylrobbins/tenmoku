//! A terminal view in a test window, running a real shell on a pty.

use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView, TerminalViewEvent};
use gpui::{
    Entity, Focusable, Hsla, Pixels, Size, TestAppContext, VisualTestContext, px, rgb, size,
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

/// The last size the shell printed, as `stty size` says it: rows, then
/// columns.
fn last_size(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<String> {
    let text = view.read_with(cx, |view, _| view.terminal().map(|t| t.snapshot().text()))?;
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix("size "))
        .map(str::to_owned)
}

/// The `stty size` a window of `window` should give: ⌊height / cell
/// height⌋ rows by ⌊width / cell width⌋ columns, where a cell is the advance
/// of `m` wide and 1.25 × the font size, rounded, high.
fn expected(cx: &mut VisualTestContext, window: Size<Pixels>) -> String {
    let style = style();
    let cell_width = cx.update(|window, _| {
        let text_system = window.text_system();
        let font = text_system.resolve_font(&gpui::font(style.font_family.clone()));
        text_system
            .advance(font, style.font_size, 'm')
            .unwrap()
            .width
    });
    let cell_height = (style.font_size * 1.25).round();
    let rows = (f32::from(window.height) / f32::from(cell_height)).floor();
    let cols = (f32::from(window.width) / f32::from(cell_width)).floor();
    format!("{rows} {cols}")
}

#[gpui::test]
fn sizes_the_pty(cx: &mut TestAppContext) {
    // The pty's threads wake the view's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    // Prints `stty size` whenever it changes, and exits 3 once `done` exists.
    let script = r#"prev=
while :; do
  s=$(stty size)
  if [ "$s" != "$prev" ]; then echo "size $s"; prev=$s; fi
  [ -e done ] && exit 3
  sleep 0.02
done"#;
    let mut options = TerminalOptions::new(dir.path());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), script.to_owned()],
    ));
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    let events = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        let events = events.clone();
        cx.subscribe(&view, move |_, event: &TerminalViewEvent, _| {
            events.borrow_mut().push(event.clone());
        })
        .detach();
    });

    let window = size(px(800.), px(400.));
    cx.simulate_resize(window);
    let want = expected(cx, window);
    until(cx, &format!("stty size {want}"), |cx| {
        last_size(&view, cx).as_deref() == Some(want.as_str())
    });
    let grid = view.read_with(cx, |view, _| view.grid_size()).unwrap();
    assert_eq!(format!("{} {}", grid.rows, grid.cols), want);

    let window = size(px(500.), px(250.));
    cx.simulate_resize(window);
    let resized = expected(cx, window);
    assert_ne!(resized, want);
    until(cx, &format!("stty size {resized}"), |cx| {
        last_size(&view, cx).as_deref() == Some(resized.as_str())
    });

    assert!(events.borrow().is_empty());
    std::fs::write(dir.path().join("done"), "").unwrap();
    until(cx, "the shell to exit", |_| !events.borrow().is_empty());
    assert_eq!(*events.borrow(), [TerminalViewEvent::Exited(Some(3))]);
}

#[gpui::test]
fn unfocused_while_the_window_is_inactive(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let options = TerminalOptions::new(dir.path());
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    let focused =
        |cx: &mut VisualTestContext| cx.update(|window, cx| view.read(cx).is_focused(window));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.read(cx).focus_handle(cx), cx)
    });
    cx.run_until_parked();
    assert!(focused(cx));
    cx.deactivate_window();
    assert!(!focused(cx));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert!(focused(cx));
}
