//! The terminal panel's find row: `terminal.find`, typing, the find keys,
//! the toggles, and the query following the active terminal.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{TerminalQuery, TerminalView};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, SharedString, TestAppContext,
    VisualTestContext, point,
};
use kiln_shell::DockView as _;

use super::TERMINAL;
use crate::{Launch, TerminalPanel, Workbench, find_bar::FindOp, tests::open};

/// What the test shell prints: `foo` three times, newest last, if case is
/// ignored, then waits.
const SCRIPT: &str = r"printf 'Foo bar\r\nxfoo\r\nfoo\r\nend\r\n'; read line";

/// What the shell prints for selecting: a short line, then one of `z`s
/// that soft-wraps onto the next row.
const WRAPPING: &str = "printf 'alpha beta\\r\\n'; cols=$(stty size | cut -d' ' -f2); \
    head -c $((cols + 5)) /dev/zero | tr '\\0' z; printf '\\r\\nend\\r\\n'; read line";

/// A workbench showing `a.rs` from a temporary project, focused as the app
/// focuses it, whose terminals run [`SCRIPT`].
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    // The pty's threads wake the terminal's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n")))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", SCRIPT]);
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

/// Runs workbench command `id` as an unbound key or the palette would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Runs `id` from the focused find row's input, as a key bound there
/// would.
fn run_in_row(id: &str, cx: &mut VisualTestContext) {
    let context: ContextStack = ["workbench", "find_bar", "text_input"]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn active_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| terminal.view().unwrap().clone())
}

/// Draws until the active session's shell has printed [`SCRIPT`]'s lines,
/// then once more so the find matches cover them.
/// Panics after 5 s.
fn printed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    let view = active_view(workbench, cx);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        let done = view.read_with(cx, |view, _| {
            (view.terminal()).is_some_and(|t| t.snapshot().text().contains("end"))
        });
        if done {
            // The matches are found in prepaint: the text may have landed
            // after the draw above, so draw once more with it on screen.
            draw(cx);
            return view;
        }
        assert!(Instant::now() < deadline, "timed out waiting for the shell");
        thread::sleep(Duration::from_millis(10));
    }
}

fn row_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| {
        terminal.find().is_some_and(|find| find.is_open())
    })
}

fn input_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let terminal = terminal(workbench, cx);
    let input = terminal.read_with(cx, |terminal, _| terminal.find().unwrap().input().clone());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

fn query(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<TerminalQuery> {
    view.read_with(cx, |view, _| view.find_query().cloned())
}

/// The selection's first cell on screen: (row, column).
fn selected(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<(usize, usize)> {
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        (snapshot.selection.first()).map(|span| (span.row, span.start_col))
    })
}

/// The rows of the matches the active terminal highlights.
fn highlighted(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Vec<i32> {
    view.read_with(cx, |view, _| {
        (view.find_matches().iter())
            .map(|found| found.start.line)
            .collect()
    })
}

/// What the row shows beside the input, and whether it's an error.
fn label(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(SharedString, bool)> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, cx| {
        let view = terminal.view().map(|view| view.read(cx));
        terminal.find().unwrap().label(view)
    })
}

#[gpui::test]
fn show_type_close(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);

    // From the editor, with the dock hidden: the dock shows with a session
    // and the row open, focus in its input.
    run("terminal.find", cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
    assert!(row_open(&workbench, cx));
    assert!(input_focused(&workbench, cx));
    let view = printed(&workbench, cx);
    assert_eq!(query(&view, cx), None);

    // Typing sets the query, smart case, and selects the newest match.
    type_text("foo", cx);
    assert_eq!(query(&view, cx), Some(TerminalQuery::literal("foo")));
    assert_eq!(selected(&view, cx), Some((2, 0)));
    assert_eq!(highlighted(&view, cx), [0, 1, 2]);
    assert_eq!(label(&workbench, cx), None);
    assert!(input_focused(&workbench, cx));

    // Escape, through the keymap: the query clears, the row closes and the
    // terminal has focus.
    keys("escape", cx);
    assert_eq!(query(&view, cx), None);
    assert!(highlighted(&view, cx).is_empty());
    assert!(!row_open(&workbench, cx));
    assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn steps_and_wraps(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    // The pane's find bar, with a query, closed.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.run_find(FindOp::Show, window, cx)));
    draw(cx);
    type_text("fn", cx);
    keys("escape", cx);
    let pane_bar = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, cx| {
            let bar = pane.find_bar().unwrap();
            (bar.is_open(), bar.query(cx).text)
        })
    };
    assert_eq!(pane_bar(cx), (false, "fn".to_owned()));

    run("terminal.find", cx);
    let view = printed(&workbench, cx);
    type_text("foo", cx);
    assert_eq!(highlighted(&view, cx), [0, 1, 2]);
    assert_eq!(selected(&view, cx), Some((2, 0)));

    // Enter steps up, to older matches, wrapping from the oldest to the
    // newest.
    let rows = |keys_: &str, cx: &mut VisualTestContext| {
        keys(keys_, cx);
        selected(&view, cx).unwrap().0
    };
    assert_eq!(rows("enter", cx), 1);
    assert_eq!(rows("enter", cx), 0);
    assert_eq!(rows("enter", cx), 2);
    // Shift-enter steps down, wrapping from the newest to the oldest.
    assert_eq!(rows("shift-enter", cx), 0);
    assert_eq!(rows("shift-enter", cx), 1);
    assert_eq!(rows("shift-enter", cx), 2);
    assert!(input_focused(&workbench, cx));
    assert_eq!(pane_bar(cx), (false, "fn".to_owned()));
}

#[gpui::test]
fn toggles_and_errors(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.find", cx);
    let first = printed(&workbench, cx);

    // Literal, `fo(` matches nothing.
    type_text("fo(", cx);
    assert_eq!(label(&workbench, cx), Some(("No results".into(), false)));
    assert!(highlighted(&first, cx).is_empty());

    // As a regex it's invalid: its error shows and nothing is highlighted.
    run_in_row("find.toggle_regex", cx);
    let (error, is_error) = label(&workbench, cx).expect("an error");
    assert!(is_error);
    assert!(!error.is_empty());
    assert_eq!(query(&first, cx), None);
    assert!(highlighted(&first, cx).is_empty());

    // `fo+` as a regex matches `foo`, and smart case `Foo` too.
    keys("backspace", cx);
    type_text("+", cx);
    assert_eq!(query(&first, cx), Some(TerminalQuery::regex("fo+")));
    assert_eq!(label(&workbench, cx), None);
    assert_eq!(highlighted(&first, cx), [0, 1, 2]);
    assert_eq!(selected(&first, cx), Some((2, 0)));

    // Match case on: `Foo` no longer matches.
    run_in_row("find.toggle_case", cx);
    let case_sensitive = query(&first, cx).unwrap().case_sensitive;
    assert_eq!(case_sensitive, Some(true));
    assert_eq!(highlighted(&first, cx), [1, 2]);
    // Whole word does nothing here.
    run_in_row("find.toggle_whole_word", cx);
    assert_eq!(highlighted(&first, cx), [1, 2]);

    // A new terminal takes the query, then `terminal.next` moves it back,
    // clearing it on the one left.
    run("terminal.new", cx);
    let second = printed(&workbench, cx);
    assert_eq!(query(&first, cx), None);
    assert_eq!(query(&second, cx).unwrap().text, "fo+");
    assert_eq!(highlighted(&second, cx), [1, 2]);
    run("terminal.next", cx);
    assert_eq!(active_view(&workbench, cx), first);
    assert_eq!(query(&second, cx), None);
    assert!(highlighted(&second, cx).is_empty());
    assert_eq!(query(&first, cx).unwrap().text, "fo+");
    assert_eq!(highlighted(&first, cx), [1, 2]);
    assert!(row_open(&workbench, cx));

    // The last session going closes the row.
    run("terminal.close", cx);
    run("terminal.close", cx);
    assert!(!row_open(&workbench, cx));
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn dock_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

#[gpui::test]
fn focus_in_the_row_is_the_terminals(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.find", cx);
    let view = printed(&workbench, cx);
    assert!(input_focused(&workbench, cx));

    // `terminal.toggle` from the row hides the dock and focuses the
    // editor, as from the terminal.
    keys("ctrl-`", cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));

    // Shown again: the terminal has focus, the row still open.
    keys("ctrl-`", cx);
    assert!(dock_shown(&workbench, cx));
    assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
    assert!(row_open(&workbench, cx));

    // The dock's rail hiding it with focus in the row: the editor has
    // focus.
    run("terminal.find", cx);
    assert!(input_focused(&workbench, cx));
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.update_in(cx, |dock, window, cx| {
        dock.rail_clicked(TERMINAL, window, cx)
    });
    draw(cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

#[gpui::test]
fn errors_follow_sessions(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.find", cx);
    let first = printed(&workbench, cx);
    run_in_row("find.toggle_regex", cx);
    type_text("fo(", cx);
    let error = label(&workbench, cx).expect("an error");
    assert!(error.1);

    // A new session, before and after its shell starts: the error stays and
    // nothing is found there.
    run("terminal.new", cx);
    assert_eq!(label(&workbench, cx), Some(error.clone()));
    let second = printed(&workbench, cx);
    assert_eq!(label(&workbench, cx), Some(error.clone()));
    assert_eq!(query(&second, cx), None);
    assert!(highlighted(&second, cx).is_empty());
    assert_eq!(query(&first, cx), None);

    // With every session gone the row closes; `terminal.find` starts one
    // and, once its shell has, shows the kept query's error.
    run("terminal.close", cx);
    run("terminal.close", cx);
    assert!(!row_open(&workbench, cx));
    run("terminal.find", cx);
    let third = printed(&workbench, cx);
    assert!(row_open(&workbench, cx));
    assert_eq!(label(&workbench, cx), Some(error));
    assert_eq!(query(&third, cx), None);
    assert!(highlighted(&third, cx).is_empty());
}

/// The centre of the cell at `row`, `col` of `view`, as last painted, with
/// `frac` of the cell's width to its left.
fn cell(
    view: &Entity<TerminalView>,
    row: usize,
    col: usize,
    frac: f32,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let style = view.read_with(cx, |view, _| view.style().clone());
    let size = cx.update(|window, _| fe_terminal_view::cell_size(&style, window));
    let origin = view.read_with(cx, |view, _| {
        // A cell each, the first `alpha`'s `a`, top left.
        let first = view.painted().text.first().expect("text was painted");
        assert_eq!(first.text, "a");
        first.origin
    });
    origin
        + point(
            size.width * (col as f32 + frac),
            size.height * (row as f32 + 0.5),
        )
}

fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
}

fn input_text(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, cx| {
        let input = terminal.find().unwrap().input();
        input.read(cx).text().to_owned()
    })
}

#[gpui::test]
fn a_selection_on_one_row_is_the_query(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    terminal(&workbench, cx).update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", WRAPPING]);
    });
    keys("ctrl-`", cx);
    let view = printed(&workbench, cx);
    view.update(cx, |view, _| view.record_paint(true));
    draw(cx);

    // `beta`, on the first row.
    drag(cell(&view, 0, 6, 0.2, cx), cell(&view, 0, 9, 0.8, cx), cx);
    run("terminal.find", cx);
    assert_eq!(input_text(&workbench, cx), "beta");
    assert_eq!(query(&view, cx), Some(TerminalQuery::literal("beta")));
    keys("escape", cx);

    // `z`s from the second row onto the third, where the line soft-wraps:
    // the query stays.
    drag(cell(&view, 1, 0, 0.2, cx), cell(&view, 2, 2, 0.8, cx), cx);
    let selected = view.read_with(cx, |view, _| view.terminal().unwrap().selected_text());
    let selected = selected.expect("the drag selected");
    assert!(selected.starts_with("zzz") && !selected.contains('\n'));
    run("terminal.find", cx);
    assert_eq!(input_text(&workbench, cx), "beta");
}
