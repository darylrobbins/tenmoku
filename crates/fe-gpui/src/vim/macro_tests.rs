//! Macros in the editor: `q` records every key that reaches it, the text
//! typed in insert mode and the command line's included, and `@` replays
//! them through the same paths, dispatching no keystroke.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, Point};
use fe_command::Invocation;
use fe_editor::EditorModel;
use gpui::{Entity, EntityInputHandler as _, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, commands::RunCommand, init};

/// A focused editor in a window of its own showing `text`, vim on, the
/// caret at `at`. [`init`] has run.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    at: (u32, u32),
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        let offset = model.text().point_to_offset(Point::new(at.0, at.1));
        model.set_cursor(offset);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

/// The primary caret's head, as a row and a byte column.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

/// Register `name`'s text.
fn register(name: char, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        let registers = &cx.global::<Vim>().registers;
        registers.get(name).map(|register| register.text.clone())
    })
}

/// Ends the open command line with `text` and `<CR>`, as the status bar's
/// line does: tells the editor, then submits.
fn submit_line(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| view.record_command_line(text, VimKey::Enter));
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": text }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(submit)), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn record_and_replay(cx: &mut TestAppContext) {
    cx.update(init);
    counted_replay(cx);
    typed_text_and_line_breaks_replay(cx);
    command_line_replays_unseen(cx);
    command_line_records_its_prefill_and_cancel(cx);
    only_committed_text_records(cx);
    replay_dispatches_no_keystroke(cx);
    recursive_macro_stops_at_the_end(cx);
    insert_mode_arrows_record_as_vim_keys(cx);
    replace_commit_replays_alike(cx);
}

/// Typed text, then a motion, recorded; a counted replay is one undo step
/// a row.
fn counted_replay(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "a\nb\nc\nd\ne", (0, 0));
    cx.simulate_keystrokes("q a shift-a");
    assert_eq!(mode(&view, cx), Some("insert"));
    cx.simulate_input(";");
    cx.simulate_keystrokes("escape j q");
    assert_eq!(register('a', cx).as_deref(), Some("A;\x1bj"));
    assert_eq!(text(&view, cx), "a;\nb\nc\nd\ne");
    assert_eq!(caret(&view, cx), (1, 0));
    cx.simulate_keystrokes("3 @ a");
    assert_eq!(text(&view, cx), "a;\nb;\nc;\nd;\ne");
    assert_eq!(caret(&view, cx), (4, 0));
    assert_eq!(mode(&view, cx), Some("normal"));
    for undone in ["a;\nb;\nc;\nd\ne", "a;\nb;\nc\nd\ne", "a;\nb\nc\nd\ne"] {
        cx.simulate_keystrokes("u");
        assert_eq!(text(&view, cx), undone);
    }
}

fn typed_text_and_line_breaks_replay(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one\ntwo", (0, 0));
    cx.simulate_keystrokes("q b i");
    cx.simulate_input("xy");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("z");
    cx.simulate_keystrokes("backspace");
    cx.simulate_input("w");
    cx.simulate_keystrokes("escape q");
    assert_eq!(register('b', cx).as_deref(), Some("ixy\rz<BS>w\x1b"));
    assert_eq!(text(&view, cx), "xy\nwone\ntwo");
    cx.simulate_keystrokes("shift-g @ b");
    assert_eq!(text(&view, cx), "xy\nwone\nxy\nwtwo");
    assert_eq!(mode(&view, cx), Some("normal"));
    // One undo step, as typing it was.
    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), "xy\nwone\ntwo");
}

fn command_line_replays_unseen(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "x\nfoo\nbar\nfoo", (0, 0));
    cx.simulate_keystrokes("q a /");
    assert_eq!(mode(&view, cx), Some("cmdline"));
    submit_line(&view, "foo", cx);
    cx.simulate_keystrokes("q");
    assert_eq!(register('a', cx).as_deref(), Some("/foo\r"));
    assert_eq!(caret(&view, cx), (1, 0));

    // Whoever observes the editor never sees the line open.
    let modes = Rc::new(RefCell::new(Vec::new()));
    let _observe = cx.update(|_, cx| {
        let modes = modes.clone();
        cx.observe(&view, move |view, cx| {
            modes.borrow_mut().push(view.read(cx).vim_mode());
        })
    });
    cx.simulate_keystrokes("@ a");
    assert_eq!(caret(&view, cx), (3, 0));
    assert_eq!(mode(&view, cx), Some("normal"));
    let modes = modes.borrow();
    assert!(!modes.is_empty());
    assert!(!modes.contains(&Some("cmdline")), "{modes:?}");
}

fn command_line_records_its_prefill_and_cancel(cx: &mut TestAppContext) {
    // `:` after a count starts the line with `.,.+1`: a replay starts it
    // so too, so only what's typed after it is recorded, and what's taken
    // out of it as `<BS>`.
    let (view, cx) = vim_editor(cx, "a\nb\nc", (0, 0));
    cx.simulate_keystrokes("q a 2 :");
    assert_eq!(
        view.read_with(cx, |view, _| view
            .vim_state()
            .unwrap()
            .command_line_prefill()
            .to_owned()),
        ".,.+1"
    );
    view.update(cx, |view, _| view.record_command_line(".,.+d", VimKey::Esc));
    let cancel = Invocation::of("vim.command_line_cancel").unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(cancel)), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("q");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(register('a', cx).as_deref(), Some("2:<BS>d\x1b"));
}

fn only_committed_text_records(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", (0, 0));
    cx.simulate_keystrokes("q a i");
    let ime =
        |cx: &mut VisualTestContext,
         f: &dyn Fn(&mut EditorView, &mut gpui::Window, &mut gpui::Context<EditorView>)| {
            cx.update(|window, cx| view.update(cx, |view, cx| f(view, window, cx)));
        };
    ime(cx, &|view, window, cx| {
        view.replace_and_mark_text_in_range(None, "ka", Some(2..2), window, cx)
    });
    ime(cx, &|view, window, cx| {
        view.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx)
    });
    ime(cx, &|view, window, cx| {
        view.replace_text_in_range(None, "仮名", window, cx)
    });
    cx.simulate_keystrokes("escape q");
    assert_eq!(text(&view, cx), "仮名");
    assert_eq!(register('a', cx).as_deref(), Some("i仮名\x1b"));
}

fn replay_dispatches_no_keystroke(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abc\nabc\nabc", (0, 0));
    cx.simulate_keystrokes("q a shift-a");
    cx.simulate_input("!?");
    cx.simulate_keystrokes("escape j 0 x q");
    assert_eq!(register('a', cx).as_deref(), Some("A!?\x1bj0x"));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let _intercept = cx.update(|_, cx| {
        let seen = seen.clone();
        cx.intercept_keystrokes(move |event, _, _| {
            seen.borrow_mut().push(event.keystroke.unparse());
        })
    });
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "abc!?\nbc!?\nbc");
    assert_eq!(
        seen.borrow().len(),
        2,
        "only `@` and `a`: {:?}",
        seen.borrow()
    );
}

fn recursive_macro_stops_at_the_end(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "a\nb\nc\nd", (0, 0));
    // An empty `a`, then a macro that plays itself after `j`.
    cx.simulate_keystrokes("q a q");
    cx.simulate_keystrokes("q a shift-a");
    cx.simulate_input(";");
    cx.simulate_keystrokes("escape j @ a q");
    assert_eq!(register('a', cx).as_deref(), Some("A;\x1bj@a"));
    assert_eq!(text(&view, cx), "a;\nb\nc\nd");
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "a;\nb;\nc;\nd;");
    assert_eq!(caret(&view, cx), (3, 1));
    // The replay is over: keys are typed again.
    cx.simulate_keystrokes("x");
    assert_eq!(text(&view, cx), "a;\nb;\nc;\nd");
}

fn insert_mode_arrows_record_as_vim_keys(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "ab\nab", (0, 0));
    cx.simulate_keystrokes("q a i");
    cx.simulate_input("x");
    cx.simulate_keystrokes("right");
    cx.simulate_input("y");
    // `alt-z` (soft wrap) has no vim key: left out.
    cx.simulate_keystrokes("alt-z alt-z escape q");
    assert_eq!(register('a', cx).as_deref(), Some("ix<Right>y\x1b"));
    assert_eq!(text(&view, cx), "xayb\nab");
    cx.simulate_keystrokes("j 0 @ a");
    assert_eq!(text(&view, cx), "xayb\nxayb");
    // In normal mode the arrows are `vim.move`s from the keymap.
    cx.simulate_keystrokes("0 q c right x q");
    assert_eq!(register('c', cx).as_deref(), Some("<Right>x"));
    assert_eq!(text(&view, cx), "xayb\nxyb");
    cx.simulate_keystrokes("@ c");
    assert_eq!(text(&view, cx), "xayb\nxy");
}

/// Text committed at once in replace mode, a ZWJ sequence among it,
/// overwrites as its replay does, a character at a time.
fn replace_commit_replays_alike(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "🚀🚀🚀🚀", (0, 0));
    cx.simulate_keystrokes("q a shift-r");
    // One commit, as an input method makes it.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_text_in_range(None, "👩\u{200d}🚀", window, cx)
        })
    });
    cx.simulate_keystrokes("escape q");
    assert_eq!(register('a', cx).as_deref(), Some("R👩\u{200d}🚀\x1b"));
    let typed = text(&view, cx);
    cx.simulate_keystrokes("u 0");
    assert_eq!(text(&view, cx), "🚀🚀🚀🚀");
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), typed);
}
