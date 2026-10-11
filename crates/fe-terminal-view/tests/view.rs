//! A terminal view in a test window, running a real shell on a pty.

use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{
    PaintedCursor, PaintedText, TerminalOptions, TerminalStyle, TerminalView, TerminalViewEvent,
};
use gpui::{
    Bounds, Entity, Focusable, Hsla, Pixels, Size, TestAppContext, VisualTestContext, point, px,
    rgb, size,
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

/// A view whose shell prints `output` (shell `printf` syntax), then waits.
/// Returns once the grid shows `ready`.
fn painting<'a>(
    cx: &'a mut TestAppContext,
    output: &str,
    ready: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let mut options = TerminalOptions::new(dir.keep());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), format!("printf '{output}'; read line")],
    ));
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    cx.simulate_resize(size(px(400.), px(200.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.read(cx).focus_handle(cx), cx)
    });
    until(cx, ready, |cx| {
        view.read_with(cx, |view, _| {
            view.terminal()
                .is_some_and(|terminal| terminal.snapshot().text().contains(ready))
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

fn cell(cx: &mut VisualTestContext) -> Size<Pixels> {
    cx.update(|window, _| fe_terminal_view::cell_size(&style(), window))
}

/// The origins' columns and rows, each checked to be on the grid.
fn on_grid(texts: &[PaintedText], cell: Size<Pixels>) -> Vec<(usize, usize, String)> {
    texts
        .iter()
        .map(|text| {
            let col = f32::from(text.origin.x) / f32::from(cell.width);
            let row = f32::from(text.origin.y) / f32::from(cell.height);
            assert!(
                (col - col.round()).abs() < 1e-3 && (row - row.round()).abs() < 1e-3,
                "{:?} is off the grid",
                text
            );
            (
                row.round() as usize,
                col.round() as usize,
                text.text.clone(),
            )
        })
        .collect()
}

#[gpui::test]
fn paints_every_cell_on_the_grid(cx: &mut TestAppContext) {
    // Narrow glyphs, a ligature-capable sequence, a combining mark, a wide
    // character, and underlined, struck-out text ending in the cursor on the
    // wide character.
    let output = "iiii\\r\\n->=>\\r\\ne\\314\\201\\r\\n\\346\\274\\242\\r\\n\
                  \\033[4;9mab\\346\\274\\242\\033[0m\\033[6;1Hdone\\033[5;3H";
    let (view, cx) = painting(cx, output, "done");
    let cell = cell(cx);
    let report = view.read_with(cx, |view, _| view.painted().clone());
    let text = on_grid(&report.text, cell);
    let row = |n: usize| {
        text.iter()
            .filter(|(row, ..)| *row == n)
            .map(|(_, col, text)| (*col, text.as_str()))
            .collect::<Vec<_>>()
    };
    // One shaped line per cell: each glyph at its column, whatever its width,
    // and no ligature across cells.
    assert_eq!(row(0), [(0, "i"), (1, "i"), (2, "i"), (3, "i")]);
    assert_eq!(row(1), [(0, "-"), (1, ">"), (2, "="), (3, ">")]);
    assert_eq!(row(2), [(0, "e\u{301}")]);
    assert_eq!(row(3), [(0, "漢")]);
    // The cursor paints its own cell's character.
    assert_eq!(row(4), [(0, "a"), (1, "b")]);

    // Underline and strikeout span the two cells `ab` occupy (the cursor draws
    // the wide character's), not the glyphs' natural width.
    let decorations = |quads: &[Bounds<Pixels>], row: usize| {
        quads
            .iter()
            .filter(|quad| quad.size.height == px(1.))
            .filter(|quad| {
                (f32::from(quad.origin.y) / f32::from(cell.height)).floor() as usize == row
            })
            .map(|quad| (quad.origin.x, quad.size.width))
            .collect::<Vec<_>>()
    };
    let spans = decorations(&report.quads, 4);
    assert_eq!(spans.len(), 2, "{spans:?}");
    for (x, width) in spans {
        assert_eq!((x, width), (px(0.), cell.width * 2.));
    }

    // The block cursor covers the wide character's two cells, and its glyph
    // and decorations are laid out the same way.
    let Some(PaintedCursor::Block(block)) = report.cursor else {
        panic!("expected a block cursor, got {:?}", report.cursor);
    };
    assert_eq!(block.origin, point(cell.width * 2., cell.height * 4.));
    assert_eq!(block.size, size(cell.width * 2., cell.height));
    assert_eq!(
        on_grid(&report.cursor_text, cell),
        [(4, 2, "漢".to_owned())]
    );
    assert_eq!(report.cursor_quads.len(), 2);
    for quad in &report.cursor_quads {
        assert_eq!(quad.origin.x, cell.width * 2.);
        assert_eq!(quad.size.width, cell.width * 2.);
    }
}

#[gpui::test]
fn an_idle_cursor_redraws_when_the_window_changes_activation(cx: &mut TestAppContext) {
    let (view, cx) = painting(cx, "idle", "idle");
    let notified = Rc::new(RefCell::new(0));
    cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&view, move |_, _| *notified.borrow_mut() += 1)
            .detach();
    });
    let cursor = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(cx, |view, _| view.painted().cursor)
    };
    assert!(matches!(cursor(cx), Some(PaintedCursor::Block(_))));

    // No terminal output in between: only the activation changes.
    cx.deactivate_window();
    cx.run_until_parked();
    assert!(*notified.borrow() >= 1, "deactivating didn't notify");
    let Some(PaintedCursor::Hollow(hollow)) = cursor(cx) else {
        panic!("expected a hollow cursor while the window is inactive");
    };

    let before = *notified.borrow();
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert!(*notified.borrow() > before, "reactivating didn't notify");
    let Some(PaintedCursor::Block(block)) = cursor(cx) else {
        panic!("expected a filled cursor once the window is active again");
    };
    assert_eq!(block, hollow);
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
