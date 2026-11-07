//! [`EditorInputEvent`]: what the host hears of typing and pasting, and
//! what it doesn't (a composition, a vim replay, another actor's paste).

use std::{cell::RefCell, rc::Rc};

use fe_buffer::Buffer;
use fe_command::{Actor, Invocation};
use fe_editor::EditorModel;
use gpui::{
    ClipboardItem, Entity, EntityInputHandler as _, Focusable as _, TestAppContext,
    VisualTestContext,
};

use super::EditorInputEvent;
use crate::{EditorTheme, EditorView, commands::RunCommand, init, set_vim_enabled};

type Heard = Rc<RefCell<Vec<EditorInputEvent>>>;

/// A focused editor showing `text`, the caret at its end, and what it tells
/// its host from then on.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    vim: bool,
) -> (Entity<EditorView>, &'a mut VisualTestContext, Heard) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(text.len());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, vim));
    let heard = Heard::default();
    let sink = heard.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &EditorInputEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach()
    });
    cx.run_until_parked();
    (view, cx, heard)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn version(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> fe_buffer::Version {
    view.read_with(cx, |view, _| view.model().buffer().version())
}

fn run(invocation: Invocation, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

fn typed(text: &str, view: &Entity<EditorView>, cx: &mut VisualTestContext) -> EditorInputEvent {
    EditorInputEvent::Typed {
        text: text.into(),
        version: version(view, cx),
    }
}

#[gpui::test]
fn typing_and_pasting_are_heard_but_not_a_composition(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "fn f() {\n", false);

    cx.simulate_input("}");
    assert_eq!(text(&view, cx), "fn f() {\n}");
    assert_eq!(heard.take(), [typed("}", &view, cx)]);

    // A composition, as it composes and as it commits, is not typing.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "ka", None, window, cx);
            view.replace_text_in_range(None, "}", window, cx);
        })
    });
    assert_eq!(text(&view, cx), "fn f() {\n}}");
    assert_eq!(heard.take(), []);

    // A paste spans what it put in.
    let before = text(&view, cx).len();
    cx.write_to_clipboard(ClipboardItem::new_string("a\nb\n".into()));
    run(Invocation::of("editor.paste").unwrap(), cx);
    assert_eq!(text(&view, cx), "fn f() {\n}}a\nb\n");
    assert_eq!(
        heard.take(),
        [EditorInputEvent::Pasted {
            range: before..before + 4,
            rows: 2,
            version: version(&view, cx),
        }]
    );

    // Another actor's paste isn't the user's; a paste of nothing changes
    // nothing.
    let agent = Actor::Ai {
        agent: "a".into(),
        session: "s".into(),
    };
    run(Invocation::of("editor.paste").unwrap().actor(agent), cx);
    assert_eq!(text(&view, cx), "fn f() {\n}}a\nb\na\nb\n");
    cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
    run(Invocation::of("editor.paste").unwrap(), cx);
    assert_eq!(heard.take(), []);
}

#[gpui::test]
fn vim_puts_are_heard_but_not_replays(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "one\ntwo", true);
    cx.simulate_keystrokes("g g 0");

    // Typed in insert mode, while recording.
    cx.simulate_keystrokes("q a i");
    cx.simulate_input("}");
    assert_eq!(heard.take(), [typed("}", &view, cx)]);
    cx.simulate_keystrokes("escape q");
    // The replay types it again, unheard.
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "}}one\ntwo");
    assert_eq!(heard.take(), []);

    // `yyp` puts a row below.
    cx.simulate_keystrokes("y y p");
    assert_eq!(text(&view, cx), "}}one\n}}one\ntwo");
    assert_eq!(
        heard.take(),
        [EditorInputEvent::Pasted {
            range: 6..11,
            rows: 1,
            version: version(&view, cx),
        }]
    );
    // `.` repeats it as a replay.
    cx.simulate_keystrokes(".");
    assert_eq!(text(&view, cx), "}}one\n}}one\n}}one\ntwo");
    assert_eq!(heard.take(), []);
}

#[gpui::test]
fn enter_is_a_line_break_typed(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "f();", false);
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&view, cx), "f();\n");
    assert_eq!(heard.take(), [typed("\n", &view, cx)]);
    // Run as a command by another actor, it isn't.
    let agent = Actor::Ai {
        agent: "a".into(),
        session: "s".into(),
    };
    run(Invocation::of("editor.newline").unwrap().actor(agent), cx);
    assert_eq!(text(&view, cx), "f();\n\n");
    assert_eq!(heard.take(), []);
}

#[gpui::test]
fn enter_in_vim_insert_mode_is_typed_but_not_replayed(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "f();", true);
    cx.simulate_keystrokes("q a A enter");
    assert_eq!(text(&view, cx), "f();\n");
    assert_eq!(heard.take(), [typed("\n", &view, cx)]);
    cx.simulate_keystrokes("escape q @ a");
    assert_eq!(text(&view, cx), "f();\n\n");
    assert_eq!(heard.take(), []);
}

/// What `keys` put in `text`, with vim on and the caret at the start.
fn put(cx: &mut TestAppContext, text_before: &str, keys: &str) -> (String, Vec<EditorInputEvent>) {
    let (view, cx, heard) = editor(cx, text_before, true);
    cx.simulate_keystrokes("g g 0");
    cx.simulate_keystrokes(keys);
    let pasted = heard.take();
    (text(&view, cx), pasted)
}

fn pasted(range: std::ops::Range<usize>, rows: u32, events: &[EditorInputEvent]) {
    match events {
        [
            EditorInputEvent::Pasted {
                range: got,
                rows: got_rows,
                ..
            },
        ] => assert_eq!((got.clone(), *got_rows), (range, rows)),
        other => panic!("not one paste: {other:?}"),
    }
}

#[gpui::test]
fn a_linewise_put_at_the_end_spans_its_rows_only(cx: &mut TestAppContext) {
    // One row below the last: the line break before it isn't in it.
    let (after, events) = put(cx, "x", "y y p");
    assert_eq!(after, "x\nx");
    pasted(2..3, 1, &events);
}

#[gpui::test]
fn a_multiline_put_at_the_end_spans_its_rows_only(cx: &mut TestAppContext) {
    let (after, events) = put(cx, "a\nb", "y j G p");
    assert_eq!(after, "a\nb\na\nb");
    pasted(4..7, 2, &events);
}

#[gpui::test]
fn a_put_above_spans_its_rows(cx: &mut TestAppContext) {
    let (after, events) = put(cx, "a\nb", "y j G P");
    assert_eq!(after, "a\na\nb\nb");
    pasted(2..5, 2, &events);
}

#[gpui::test]
fn a_visual_put_over_the_last_rows_spans_what_it_put(cx: &mut TestAppContext) {
    // Two rows put over the last row, linewise.
    let (after, events) = put(cx, "a\nb\nc", "y j G shift-v p");
    assert_eq!(after, "a\nb\na\nb");
    pasted(4..7, 2, &events);
    // One row over the last: one row.
    let (after, events) = put(cx, "a\nb", "y y G shift-v p");
    assert_eq!(after, "a\na");
    pasted(2..3, 1, &events);
}

#[gpui::test]
fn a_charwise_put_spans_its_text(cx: &mut TestAppContext) {
    let (after, events) = put(cx, "ab\ncd", "v j y $ p");
    assert_eq!(after, "abab\nc\ncd");
    pasted(2..6, 2, &events);
}

#[gpui::test]
fn a_linewise_visual_put_over_characters_is_its_rows(cx: &mut TestAppContext) {
    // Line breaks go in before and after the row, which alone is put.
    let (after, events) = put(cx, "x", "y y v p");
    assert_eq!(after, "\nx\n");
    pasted(1..2, 1, &events);
}

#[gpui::test]
fn an_empty_last_row_put_counts(cx: &mut TestAppContext) {
    // Two rows, the second empty, below the last row.
    let (after, events) = put(cx, "a\n\nz", "y j G p");
    assert_eq!(after, "a\n\nz\na\n");
    pasted(5..7, 2, &events);
    // And below a middle row, the empty row's line break left out.
    let (after, events) = put(cx, "a\n\nz", "y j p");
    assert_eq!(after, "a\na\n\n\nz");
    pasted(2..4, 2, &events);
    // Over characters too.
    let (after, events) = put(cx, "a\n\nz", "y j G v p");
    assert_eq!(after, "a\n\n\na\n\n");
    pasted(4..6, 2, &events);
}

#[gpui::test]
fn a_counted_put_counts_every_copy(cx: &mut TestAppContext) {
    let (after, events) = put(cx, "x", "y y 3 p");
    assert_eq!(after, "x\nx\nx\nx");
    pasted(2..7, 3, &events);
}

#[gpui::test]
fn a_paste_is_its_own_text_not_a_linked_mirror(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "<a>\nbody\n</a>", false);
    view.update(cx, |view, _| {
        let model = view.model_mut();
        model.set_cursor(2);
        model.set_linked_ranges(vec![1..2, 11..12]);
    });
    cx.write_to_clipboard(ClipboardItem::new_string("bc".into()));
    run(Invocation::of("editor.paste").unwrap(), cx);
    assert_eq!(text(&view, cx), "<abc>\nbody\n</abc>");
    assert_eq!(
        heard.take(),
        [EditorInputEvent::Pasted {
            range: 2..4,
            rows: 1,
            version: version(&view, cx),
        }]
    );
}

#[gpui::test]
fn a_paste_is_its_own_text_not_a_snippet_mirror(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "", false);
    view.update(cx, |view, _| {
        let snippet = fe_editor::snippet::Snippet::parse("${1:x}\n\n$1");
        view.model_mut().insert_snippet(0..0, &snippet);
    });
    assert_eq!(text(&view, cx), "x\n\nx");
    cx.write_to_clipboard(ClipboardItem::new_string("yz".into()));
    run(Invocation::of("editor.paste").unwrap(), cx);
    assert_eq!(text(&view, cx), "yz\n\nyz");
    // The primary caret's text, the mirror's, one row: not both and the
    // rows between.
    assert_eq!(
        heard.take(),
        [EditorInputEvent::Pasted {
            range: 4..6,
            rows: 1,
            version: version(&view, cx),
        }]
    );
}

/// GPUI has a primary selection on Linux and FreeBSD only.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[gpui::test]
fn a_middle_click_paste_is_heard(cx: &mut TestAppContext) {
    let (view, cx, heard) = editor(cx, "fn f() {\n}\n", false);
    view.update(cx, |view, _| view.set_primary_selection(true));
    cx.run_until_parked();
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("a();\nb();\n".into())));
    let position = view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(9).unwrap();
        gpui::point(caret.left() + gpui::px(1.), caret.center().y)
    });
    cx.simulate_mouse_down(position, gpui::MouseButton::Middle, gpui::Modifiers::none());
    cx.simulate_mouse_up(position, gpui::MouseButton::Middle, gpui::Modifiers::none());
    assert_eq!(text(&view, cx), "fn f() {\na();\nb();\n}\n");
    assert_eq!(
        heard.take(),
        [EditorInputEvent::Pasted {
            range: 9..19,
            rows: 2,
            version: version(&view, cx),
        }]
    );
}

#[gpui::test]
fn characters_put_over_a_block_span_its_rows(cx: &mut TestAppContext) {
    let (after, events) = put(cx, "ab\ncd", "y l ctrl-v j p");
    assert_eq!(after, "ab\nad");
    pasted(0..4, 2, &events);
}

#[gpui::test]
fn a_block_with_an_empty_last_row_counts_it(cx: &mut TestAppContext) {
    // Over whole lines.
    let (after, events) = put(cx, "ab\n\ncd", "ctrl-v j y G shift-v p");
    assert_eq!(after, "ab\n\na\n");
    pasted(4..6, 2, &events);
    // Put in normal mode.
    let (after, events) = put(cx, "ab\n\ncd", "ctrl-v j y G $ p");
    pasted_rows(2, &events);
    assert!(after.starts_with("ab\n\ncd"), "{after:?}");
}

fn pasted_rows(rows: u32, events: &[EditorInputEvent]) {
    match events {
        [EditorInputEvent::Pasted { rows: got, .. }] => assert_eq!(*got, rows),
        other => panic!("not one paste: {other:?}"),
    }
}
