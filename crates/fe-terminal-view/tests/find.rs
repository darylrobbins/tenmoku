//! Find in a terminal view: highlighted matches and stepping through them.

use fe_terminal_view::TerminalStyle;
use fe_terminal_view::{PaintReport, QueryError, TerminalOptions, TerminalQuery, TerminalView};
use gpui::{
    AppContext as _, Entity, Focusable, Hsla, TestAppContext, VisualTestContext, point, px, rgb,
    size,
};
use std::thread;
use std::time::{Duration, Instant};

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

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A view whose shell runs `script`, then waits; returns once the grid shows
/// `ready`.
fn running<'a>(
    cx: &'a mut TestAppContext,
    script: &str,
    ready: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let mut options = TerminalOptions::new(dir.keep());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), format!("{script}; read line")],
    ));
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    view.update(cx, |view, _| view.record_paint(true));
    cx.simulate_resize(size(px(400.), px(200.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.read(cx).focus_handle(cx), cx)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        let ok = view.read_with(cx, |view, _| {
            view.terminal()
                .is_some_and(|t| t.snapshot().text().contains(ready))
        });
        if ok {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {ready}");
        thread::sleep(Duration::from_millis(10));
    }
    draw(cx);
    (view, cx)
}

fn painted(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> PaintReport {
    view.read_with(cx, |view, _| view.painted().clone())
}

#[gpui::test]
fn matches_highlighted(cx: &mut TestAppContext) {
    let (view, cx) = running(cx, r"printf 'foo bar\r\nxfoo\r\nfoo\r\nend\r\n'", "end");
    let cell = cx.update(|window, _| fe_terminal_view::cell_size(&style(), window));
    assert!(painted(&view, cx).find_quads.is_empty());

    view.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::literal("foo")), cx)
    })
    .unwrap();
    draw(cx);
    let report = painted(&view, cx);
    assert_eq!(view.read_with(cx, |view, _| view.find_matches().len()), 3);
    let expected: Vec<_> = [(0, 0), (1, 1), (2, 0)]
        .into_iter()
        .map(|(row, col)| {
            gpui::Bounds::new(
                point(cell.width * col as f32, cell.height * row as f32),
                size(cell.width * 3., cell.height),
            )
        })
        .collect();
    assert_eq!(report.find_quads, expected);

    view.update(cx, |view, cx| view.set_find(None, cx)).unwrap();
    draw(cx);
    assert!(painted(&view, cx).find_quads.is_empty());
    assert!(view.read_with(cx, |view, _| view.find_matches().is_empty()));
}

#[gpui::test]
fn find_next_selects_and_scrolls(cx: &mut TestAppContext) {
    let (view, cx) = running(cx, "seq -f 'line %g' 1 200", "line 200");
    view.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::literal("line 7")), cx)
    })
    .unwrap();
    // The first match backward from the bottom is the last: line 79.
    let found = view
        .update(cx, |view, cx| view.find_next(false, cx))
        .unwrap()
        .expect("a match");
    assert!(found.start.line < 0);
    draw(cx);
    let offset = view.read_with(cx, |view, _| {
        view.terminal().unwrap().snapshot().display_offset
    });
    assert!(offset > 0);
    let report = painted(&view, cx);
    assert!(!report.find_quads.is_empty());
    let cell = cx.update(|window, _| fe_terminal_view::cell_size(&style(), window));
    let row = (found.start.line + offset as i32) as f32;
    assert!(
        report
            .quads
            .iter()
            .any(|quad| quad.origin.y == cell.height * row),
        "the match's row paints the selection"
    );

    // Stepping on through the matches wraps to a different one eventually.
    let mut seen = vec![found];
    for _ in 0..12 {
        let next = view
            .update(cx, |view, cx| view.find_next(false, cx))
            .unwrap()
            .unwrap();
        seen.push(next);
    }
    let first = seen[0];
    assert!(seen[1..].contains(&first), "wrapped back to the first");

    // Forward past the last match wraps to the first.
    view.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::literal("line 200")), cx)
    })
    .unwrap();
    let a = view
        .update(cx, |view, cx| view.find_next(true, cx))
        .unwrap();
    let b = view
        .update(cx, |view, cx| view.find_next(true, cx))
        .unwrap();
    assert!(a.is_some());
    assert_eq!(a, b, "the only match wraps onto itself");
}

#[gpui::test]
fn bad_query_and_no_session(cx: &mut TestAppContext) {
    let (view, cx) = running(cx, "printf 'foo\\r\\nend\\r\\n'", "end");
    // A view that hasn't been laid out has no session yet.
    let mut options = TerminalOptions::new(tempfile::tempdir().unwrap().keep());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), "read line".to_owned()],
    ));
    let early = cx
        .update(|window, cx| cx.new(|cx| TerminalView::new(options, style(), window, cx).unwrap()));
    early.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::literal("x")), cx)
            .unwrap();
        assert_eq!(view.find_next(true, cx), Ok(None));
    });
    let err = view.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::regex("(")), cx)
    });
    assert!(matches!(err, Err(QueryError(_))));
    draw(cx);
    assert!(painted(&view, cx).find_quads.is_empty());
    assert_eq!(
        view.read_with(cx, |view, _| view.find_query().cloned()),
        None
    );
}
