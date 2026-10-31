//! What a diff view sets on an editor ([`super::host`]): read-only, with
//! vim's `E21` for a refused edit; the host's blocks between the rows; and
//! the scroll position in screen rows, with an event when it moves. Also
//! input method text over an explicit range in replace mode and while a
//! macro records, and the underline caret while composing in replace mode.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_command::{Invocation, Value};
use fe_display::layout::Rect;
use fe_editor::{EditorModel, decorations::Tint};
use fe_vim::search::{FindQuery, Pattern};
use gpui::{
    Bounds, ClipboardItem, Entity, EntityInputHandler as _, Focusable as _, Modifiers, MouseButton,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, SharedString, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};

#[path = "read_only_bar_tests.rs"]
mod read_only_bar_tests;

use super::host::E21;
use crate::{
    commands::{EditorViewEvent, HostBlock, HostBlockKind, RunCommand},
    *,
};

/// A focused editor showing `text`, the caret at `at`, vim on or off.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    at: usize,
    vim: bool,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(at);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, vim));
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

/// The text and its buffer version, which any edit, undo or redo moves.
fn state(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, fe_buffer::Version) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        (model.text().to_string(), model.snapshot().version())
    })
}

fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    view.read_with(cx, |view, _| view.model().primary_selection().range)
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

fn set_read_only(view: &Entity<EditorView>, on: bool, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.set_read_only(on, cx));
}

/// Runs `f` on the view, as the platform's input handler does.
fn ime<R>(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut EditorView, &mut gpui::Window, &mut gpui::Context<EditorView>) -> R,
) -> R {
    cx.update(|window, cx| view.update(cx, |view, cx| f(view, window, cx)))
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn read_only_refuses_edits(cx: &mut TestAppContext) {
    const TEXT: &str = "alpha beta\n\tgamma\ndelta\n";
    let (view, cx) = editor(cx, TEXT, 2, false);
    // An edit before, which stays the one undo would take.
    view.update(cx, |view, _| view.model_mut().insert("X"));
    let before = state(&view, cx);
    set_read_only(&view, true, cx);
    assert!(view.read_with(cx, |view, _| view.is_read_only()));
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("PASTE".into())));

    // Typing, paste, backspace.
    cx.simulate_input("typed");
    cx.simulate_keystrokes("cmd-v backspace enter tab");
    assert_eq!(state(&view, cx), before);
    // An input method's composition and commits, over the carets and over
    // a range of its own.
    ime(&view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "かな", None, window, cx)
    });
    assert_eq!(
        ime(&view, cx, |view, window, cx| view
            .marked_text_range(window, cx)),
        None
    );
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "仮名", window, cx)
    });
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(Some(0..2), "Z", window, cx)
    });
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    assert_eq!(state(&view, cx), before);

    // Every editor command that edits, over a selection, and completion.
    view.update(cx, |view, _| view.model_mut().select_range(3..9, false));
    let editing: Vec<_> = (commands::command_list().into_iter())
        .filter(|meta| meta.mutates_buffer && meta.id.as_str().starts_with("editor."))
        .collect();
    assert!(editing.len() > 10, "{editing:?}");
    let completion = [completion_menu::TRIGGER, completion_menu::ACCEPT];
    let ids = (editing.iter().map(|meta| meta.id.as_str())).chain(completion);
    for id in ids {
        let mut invocation = Invocation::of(id).unwrap();
        if id == "editor.insert" {
            invocation = invocation.args(Value::from_iter([("text", "x")]));
        }
        view.update(cx, |view, cx| view.run_command(invocation, None, cx));
        assert_eq!(state(&view, cx), before, "{id}");
    }

    // A middle click's paste of the primary selection.
    draw(cx);
    let middle = view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(0).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    });
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("PRIMARY".into())));
    cx.simulate_mouse_down(middle, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_up(middle, MouseButton::Middle, Modifiers::none());
    assert_eq!(state(&view, cx), before);

    // Moving, selecting, copying and finding work.
    view.update(cx, |view, _| view.model_mut().set_cursor(0));
    cx.simulate_keystrokes("right right");
    assert_eq!(selection(&view, cx), 2..2);
    cx.simulate_keystrokes("shift-right shift-right shift-right");
    assert_eq!(selection(&view, cx), 2..5);
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some(&before.0[2..5]));
    let pattern = Pattern::from_query(&FindQuery {
        text: "delta".into(),
        match_case: true,
        ..FindQuery::default()
    })
    .unwrap();
    view.update(cx, |view, cx| view.set_find(Some(pattern), cx));
    assert!(view.update(cx, |view, cx| view.find_next(cx)));
    let delta = before.0.find("delta").unwrap();
    assert_eq!(selection(&view, cx), delta..delta + 5);
    assert_eq!(state(&view, cx), before);

    // Editable again: the history is as it was, the one edit on it.
    set_read_only(&view, false, cx);
    cx.simulate_input("y");
    assert_ne!(text(&view, cx), before.0);
    view.update(cx, |view, _| {
        let model = view.model_mut();
        model.undo().unwrap();
        model.undo().unwrap();
    });
    assert_eq!(text(&view, cx), TEXT);
    let more = view.update(cx, |view, _| view.model_mut().undo().unwrap());
    assert_eq!(more, None);
}

/// Vim's message, if any.
fn message(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        view.vim_state()
            .and_then(|state| state.message())
            .map(str::to_owned)
    })
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

/// Opens vim's line with `key` and submits `text` on it, as the host's
/// line does.
fn submit(key: &str, text: &str, cx: &mut VisualTestContext) {
    if !key.is_empty() {
        cx.simulate_keystrokes(key);
    }
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(Value::from_iter([("text", text)]));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

fn register(name: char, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        vim_registers(cx)
            .and_then(|registers| registers.get(name))
            .map(|register| register.text.clone())
    })
}

#[gpui::test]
fn read_only_vim(cx: &mut TestAppContext) {
    const TEXT: &str = "foo bar\nbaz foo\nqux\n";
    let (view, cx) = editor(cx, TEXT, 0, true);
    // Something to put, a macro that edits, undone, and one that moves.
    cx.simulate_keystrokes("y w q a j x q u g g q b w q g g");
    assert_eq!(text(&view, cx), TEXT);
    set_read_only(&view, true, cx);
    let before = state(&view, cx);
    let refused = |keys: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        assert_eq!(state(&view, cx), before, "{keys}");
        assert_eq!(mode(&view, cx), Some("normal"), "{keys}");
        assert_eq!(message(&view, cx).as_deref(), Some(E21), "{keys}");
    };
    // `i` stays in normal mode, so `x` is vim's, and refused too.
    refused("i", cx);
    refused("x", cx);
    for keys in [
        "d d", "p", "o", "shift-o", "shift-r", "c w", "shift-a", "u", "ctrl-a", ".",
    ] {
        refused(keys, cx);
    }
    // Ex lines that edit.
    for line in ["s/foo/X/", "d", "normal x", "g/foo/d"] {
        submit(":", line, cx);
        assert_eq!(state(&view, cx), before, ":{line}");
        assert_eq!(mode(&view, cx), Some("normal"), ":{line}");
        assert_eq!(message(&view, cx).as_deref(), Some(E21), ":{line}");
    }
    // A macro runs until its first edit, which is refused.
    refused("@ a", cx);
    assert_eq!(caret_row(&view, cx), 1);
    cx.simulate_keystrokes("g g 0 @ b");
    assert_eq!((head(&view, cx), message(&view, cx)), (4, None));
    cx.simulate_keystrokes("g g 0");
    // A search an operator waits on edits: refused, as is a yank's not.
    for operator in ["d", "c", ">", "g shift-u"] {
        submit(&format!("{operator} /"), "foo", cx);
        assert_eq!(state(&view, cx), before, "{operator}/");
        assert_eq!(mode(&view, cx), Some("normal"), "{operator}/");
        assert_eq!(message(&view, cx).as_deref(), Some(E21), "{operator}/");
    }
    submit("y /", "foo", cx);
    assert_eq!(message(&view, cx), None);
    assert_eq!(register('"', cx).as_deref(), Some("foo bar\nbaz "));
    // `:normal`, alone or under `:g`, runs keys that don't edit.
    cx.simulate_keystrokes("g g");
    submit(":", "normal j", cx);
    assert_eq!((caret_row(&view, cx), message(&view, cx)), (1, None));
    submit(":", "normal yy", cx);
    assert_eq!(register('"', cx).as_deref(), Some("baz foo\n"));
    submit(":", "g/qux/normal 0", cx);
    assert_eq!((caret_row(&view, cx), message(&view, cx)), (2, None));
    assert_eq!(state(&view, cx), before);
    cx.simulate_keystrokes("g g");

    // Motions, visual mode and its yank, searches and ex lines that don't
    // edit work.
    cx.simulate_keystrokes("j");
    assert_eq!(head(&view, cx), 8);
    assert_eq!(message(&view, cx), None);
    cx.simulate_keystrokes("w");
    assert_eq!(head(&view, cx), 12);
    cx.simulate_keystrokes("v");
    assert_eq!(mode(&view, cx), Some("visual"));
    cx.simulate_keystrokes("e y");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(register('"', cx).as_deref(), Some("foo"));
    cx.simulate_keystrokes("g g");
    submit("/", "foo", cx);
    assert_eq!(head(&view, cx), 12);
    assert_eq!(mode(&view, cx), Some("normal"));
    submit(":", "3", cx);
    assert_eq!(caret_row(&view, cx), 2);
    assert_eq!(message(&view, cx), None);
    assert_eq!(state(&view, cx), before);

    // Made read-only in insert mode, the session ends.
    set_read_only(&view, false, cx);
    cx.simulate_keystrokes("i");
    assert_eq!(mode(&view, cx), Some("insert"));
    set_read_only(&view, true, cx);
    assert_eq!(mode(&view, cx), Some("normal"));
    cx.simulate_input("typed");
    assert_eq!(state(&view, cx), before);
    // An input method's text and a middle click's paste say E21 too, as
    // does nothing put over a range, which would take it out.
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "z", window, cx)
    });
    assert_eq!(message(&view, cx).as_deref(), Some(E21));
    cx.simulate_keystrokes("l");
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(Some(0..1), "", window, cx)
    });
    assert_eq!(message(&view, cx).as_deref(), Some(E21));
    assert_eq!(state(&view, cx), before);
    cx.simulate_keystrokes("l");
    assert_eq!(message(&view, cx), None);
    draw(cx);
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let middle = on_row(&view, 0, cx);
        cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("PRIMARY".into())));
        cx.simulate_mouse_down(middle, MouseButton::Middle, Modifiers::none());
        cx.simulate_mouse_up(middle, MouseButton::Middle, Modifiers::none());
        assert_eq!(message(&view, cx).as_deref(), Some(E21));
    }
    assert_eq!(state(&view, cx), before);
}

/// Made read-only in an insert or replace session, the session ends as
/// `Esc` ends it, while still editable: a count's copies and a block's rows
/// go in. A search line an operator waits on, opened before, is refused.
#[gpui::test]
fn read_only_ends_sessions_first(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab\ncd\n", 0, true);
    cx.simulate_keystrokes("3 i");
    cx.simulate_input("X");
    set_read_only(&view, true, cx);
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(text(&view, cx), "XXXab\ncd\n");

    set_read_only(&view, false, cx);
    cx.simulate_keystrokes("shift-r");
    cx.simulate_input("Y");
    set_read_only(&view, true, cx);
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(text(&view, cx), "XXYab\ncd\n");

    set_read_only(&view, false, cx);
    cx.simulate_keystrokes("g g 0 ctrl-v j shift-i");
    cx.simulate_input("Z");
    set_read_only(&view, true, cx);
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(text(&view, cx), "ZXXYab\nZcd\n");

    set_read_only(&view, false, cx);
    cx.simulate_keystrokes("g g d /");
    set_read_only(&view, true, cx);
    let before = state(&view, cx);
    submit("", "cd", cx);
    assert_eq!(state(&view, cx), before);
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(message(&view, cx).as_deref(), Some(E21));

    // With vim off, a refused input method's text says nothing.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "z", window, cx)
    });
    assert_eq!(state(&view, cx), before);
    assert_eq!(message(&view, cx), None);
}

/// `count` rows `r0` to `r{count-1}`.
fn rows(count: u32) -> String {
    (0..count).map(|row| format!("r{row}\n")).collect()
}

/// The top of display row `row` in the last frame.
fn row_top(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.row_top(row)
    })
}

/// The window position a little into the text on display row `row`.
fn on_row(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let rect = Rect {
            x: frame.text_left + 3.,
            y: frame.row_top(row),
            width: 1.,
            height: frame.line_height,
        };
        layout.bounds(rect).center()
    })
}

fn click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn caret_row(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| {
        let model = view.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    })
}

fn painted_blocks(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<element::PaintedBlock> {
    draw(cx);
    view.read_with(cx, |view, _| view.painted_blocks.clone())
}

fn filler(above: u32, rows: u32, id: u64) -> HostBlock {
    HostBlock {
        above,
        rows,
        kind: HostBlockKind::Filler,
        id,
    }
}

#[gpui::test]
fn host_blocks(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, &rows(12), 0, false);
    draw(cx);
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    let (top_3, top_6) = (row_top(&view, 3, cx), row_top(&view, 6, cx));
    assert!(painted_blocks(&view, cx).is_empty());

    let lines = HostBlockKind::Lines {
        lines: vec![SharedString::from("\tadded"), SharedString::from("two")],
        tint: Tint::Added,
        words: Vec::new(),
    };
    let blocks = vec![
        filler(3, 2, 7),
        HostBlock {
            above: 6,
            rows: 2,
            kind: lines,
            id: 9,
        },
    ];
    view.update(cx, |view, cx| view.set_blocks(blocks, cx));
    let painted = painted_blocks(&view, cx);
    assert_eq!(row_top(&view, 3, cx), top_3 + 2. * line_height);
    assert_eq!(row_top(&view, 6, cx), top_6 + 4. * line_height);

    // The filler's hatch over its two rows, just above row 3.
    assert_eq!(painted.len(), 2);
    let (hatched, text) = (&painted[0], &painted[1]);
    assert_eq!((hatched.id, hatched.band), (7, None));
    assert!(hatched.hatch > 0);
    assert_eq!(hatched.bounds.size.height, px(2. * line_height));
    let row_3 = on_row(&view, 3, cx);
    assert_eq!(hatched.bounds.bottom(), row_3.y - px(line_height / 2.));
    // The text block's lines, tabs expanded, on a band of its tint.
    assert_eq!(text.id, 9);
    let (band, change_added) = (
        text.band.unwrap(),
        view.read_with(cx, |view, _| view.theme.change_added),
    );
    assert_eq!(
        (band.h, band.s, band.l),
        (change_added.h, change_added.s, change_added.l)
    );
    assert!(band.a > 0. && band.a < change_added.a);
    let shown: Vec<_> = text.lines.iter().map(|(line, _)| line.as_str()).collect();
    assert_eq!(shown, ["    added", "two"]);
    for (ix, (_, origin)) in text.lines.iter().enumerate() {
        assert_eq!(origin.y, text.bounds.top() + px(line_height * ix as f32));
        assert!(text.bounds.contains(&(*origin + point(px(1.), px(1.)))));
    }
    assert_eq!(text.bounds.size.height, px(2. * line_height));

    // A click on the row below each block lands on it.
    for row in [3, 6] {
        click(on_row(&view, row, cx), cx);
        assert_eq!(caret_row(&view, cx), row);
    }

    // With row 6 folded away, its block goes, and so do its rows.
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(5..=7);
        cx.notify();
    });
    let painted = painted_blocks(&view, cx);
    assert_eq!(
        painted.iter().map(|block| block.id).collect::<Vec<_>>(),
        [7]
    );
    // Row 8 is display row 6 now, below the filler alone.
    assert_eq!(row_top(&view, 6, cx), top_6 + 2. * line_height);
    click(on_row(&view, 6, cx), cx);
    assert_eq!(caret_row(&view, cx), 8);

    // A filler far taller than the view hatches only what's shown of it.
    view.update(cx, |view, cx| {
        view.set_blocks(vec![filler(1, 100_000, 3)], cx)
    });
    let painted = painted_blocks(&view, cx);
    let shown = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds())
    });
    let most = (shown.size.width + shown.size.height) / px(line_height * 0.5) + 2.;
    assert!(painted[0].hatch > 0 && (painted[0].hatch as f32) < most);
}

#[gpui::test]
fn scroll_sync_api(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, &rows(500), 0, false);
    draw(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        let events = events.clone();
        cx.subscribe(&view, move |_, event: &EditorViewEvent, _| {
            events.borrow_mut().push(event.clone())
        })
        .detach();
    });
    let taken = |cx: &mut VisualTestContext| {
        draw(cx);
        cx.run_until_parked();
        events.take()
    };
    assert_eq!(taken(cx), []);

    // The wheel: three rows down.
    let at = on_row(&view, 2, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., -3.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    assert_eq!(taken(cx), [EditorViewEvent::Scrolled { top: 3. }]);
    assert_eq!(view.read_with(cx, |view, _| view.scroll_top()), 3.);

    // Set by the host: moved, but not echoed.
    view.update(cx, |view, cx| view.set_scroll_top(10.5, cx));
    assert_eq!(taken(cx), []);
    assert_eq!(view.read_with(cx, |view, _| view.scroll_top()), 10.5);
    assert_eq!(
        view.read_with(cx, |view, _| view.viewport_rows().unwrap().0),
        10
    );
    // Past the end, the frame's clamp isn't echoed either.
    view.update(cx, |view, cx| view.set_scroll_top(1e6, cx));
    assert_eq!(taken(cx), []);
    assert_eq!(view.read_with(cx, |view, _| view.scroll_top()), 500.);

    // Block rows count: three above row 5 put buffer row 20 at screen row
    // 23, where a vim scroll to it reports it.
    view.update(cx, |view, cx| {
        view.set_blocks(vec![filler(5, 3, 1)], cx);
        view.set_scroll_top(0., cx);
    });
    assert_eq!(taken(cx), []);
    view.update(cx, |view, _| view.scroll_to_top_row(20));
    view.update(cx, |_, cx| cx.notify());
    assert_eq!(taken(cx), [EditorViewEvent::Scrolled { top: 23. }]);
    assert_eq!(view.read_with(cx, |view, _| view.scroll_top()), 23.);
    view.update(cx, |view, cx| view.set_scroll_top(13., cx));
    assert_eq!(taken(cx), []);
    assert_eq!(
        view.read_with(cx, |view, _| view.viewport_rows().unwrap().0),
        10
    );
    // Inside the block, the row below it is first.
    view.update(cx, |view, cx| view.set_scroll_top(6., cx));
    assert_eq!(taken(cx), []);
    assert_eq!(
        view.read_with(cx, |view, _| view.viewport_rows().unwrap().0),
        5
    );

    // A caret moved out of view scrolls it back in, which is reported.
    view.update(cx, |view, cx| {
        let offset = view.model().text().row_start(300);
        view.select_and_reveal(offset..offset, cx)
    });
    let moved = taken(cx);
    let top = view.read_with(cx, |view, _| view.scroll_top());
    assert_eq!(moved, [EditorViewEvent::Scrolled { top }]);
    assert!(top > 250.);

    // Something else that scrolls after the host set the top, before a
    // frame, is reported: the wheel, a vim scroll, a caret to reveal.
    view.update(cx, |view, cx| view.set_scroll_top(10., cx));
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., -2.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    assert_eq!(taken(cx), [EditorViewEvent::Scrolled { top: 12. }]);
    view.update(cx, |view, cx| {
        view.set_scroll_top(30., cx);
        view.scroll_to_top_row(40);
    });
    assert_eq!(taken(cx), [EditorViewEvent::Scrolled { top: 43. }]);
    view.update(cx, |view, cx| {
        view.set_scroll_top(30., cx);
        let offset = view.model().text().row_start(200);
        view.select_and_reveal(offset..offset, cx)
    });
    let top = view.read_with(cx, |view, _| view.scroll_top());
    assert!(top > 150.);
    assert_eq!(taken(cx), [EditorViewEvent::Scrolled { top }]);
}

/// Delivers frames until no rows are pending a wrap.
fn wrap_through(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let pending = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model().display().wrap_is_pending())
    };
    let mut frames = 0;
    while pending(cx) {
        assert!(frames < 1000, "wrapping stalled");
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        frames += 1;
    }
}

/// A top partway into a block keeps its place in it while frames wrap
/// the rows around it.
#[gpui::test]
fn a_top_in_a_block_keeps_its_place_while_rows_wrap(cx: &mut TestAppContext) {
    // 1.6 MB: about a dozen wrap slices.
    let text: String = (0..4_000)
        .map(|row| format!("{row:05} {}\n", "word ".repeat(80)))
        .collect();
    let (view, cx) = editor(cx, &text, 0, false);
    cx.simulate_keystrokes("alt-z");
    wrap_through(&view, cx);
    view.update(cx, |view, cx| {
        view.set_blocks(vec![filler(2_000, 4, 1)], cx)
    });
    draw(cx);
    let block_top = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let display = view.model().display();
            let frame = &view.layout.as_ref().unwrap().frame;
            frame
                .block_map
                .screen_row(display.row_for_buffer_row(2_000)) as f32
                - 4.
        })
    };
    let top = block_top(cx) + 1.5;
    view.update(cx, |view, cx| view.set_scroll_top(top, cx));
    draw(cx);
    assert_eq!(view.read_with(cx, |view, _| view.scroll_top()), top);

    // Widen until no row wraps.
    let height = cx.update(|window, _| window.viewport_size().height);
    cx.simulate_resize(gpui::size(px(8_000.), height));
    cx.run_until_parked();
    wrap_through(&view, cx);
    draw(cx);
    let top = view.read_with(cx, |view, _| view.scroll_top());
    assert_eq!(top, block_top(cx) + 1.5);
    assert_eq!(block_top(cx), 2_000.);
}

/// A text block's line wider than the rows scrolls into view, and one too
/// long to shape whole is shaped around the view.
#[gpui::test]
fn wide_host_lines_scroll(cx: &mut TestAppContext) {
    use fe_display::layout::MAX_SHAPED_ROW_BYTES;
    let (view, cx) = editor(cx, &rows(5), 0, false);
    let wide = format!("{}END", "x".repeat(2_000));
    let long = format!("{}TAIL", "a".repeat(MAX_SHAPED_ROW_BYTES + 5_000));
    let lines = HostBlockKind::Lines {
        lines: vec![wide.clone().into(), long.clone().into()],
        tint: Tint::Modified,
        words: Vec::new(),
    };
    let block = HostBlock {
        above: 1,
        rows: 2,
        kind: lines,
        id: 4,
    };
    view.update(cx, |view, cx| view.set_blocks(vec![block], cx));
    let shown = painted_blocks(&view, cx);
    let line_texts = |shown: &[element::PaintedBlock]| -> Vec<String> {
        shown[0]
            .lines
            .iter()
            .map(|(text, _)| text.clone())
            .collect()
    };
    let texts = line_texts(&shown);
    assert_eq!(texts[0], wide);
    assert!(texts[1].len() <= MAX_SHAPED_ROW_BYTES && texts[1].starts_with('a'));
    let scroll_x = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.scroll.x)
    };
    assert_eq!(scroll_x(cx), 0.);

    // As far right as it goes: the long line's end.
    let at = on_row(&view, 0, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(-1e7), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    let shown = painted_blocks(&view, cx);
    let texts = line_texts(&shown);
    assert!(texts[1].ends_with("TAIL") && texts[1].len() <= MAX_SHAPED_ROW_BYTES);
    let far = scroll_x(cx);
    assert!(far > 0.);
    // The line's start is left of the text area, its end inside it.
    let text_right = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds()).right()
    });
    let (_, origin) = &shown[0].lines[1];
    assert!(origin.x < shown[0].bounds.left());
    assert!(origin.x < text_right);

    // Back a little: the wide line's end shows, short of the long one's.
    view.update(cx, |view, cx| {
        view.scroll.x = px(1_000.);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    assert_eq!(scroll_x(cx), 1_000.);
}

/// Text an input method commits over a range of its own while a macro
/// records, in replace mode: it overwrites from the range's start as typing
/// there does, so `<BS>` puts back what it overwrote, and the recording
/// holds it as typed. In insert mode it replaces the range.
#[gpui::test]
fn explicit_range_commits_type(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "abc", 0, true);
    cx.simulate_keystrokes("q a shift-r");
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(Some(0..1), "x", window, cx)
    });
    assert_eq!(text(&view, cx), "xbc");
    cx.simulate_keystrokes("backspace");
    assert_eq!(text(&view, cx), "abc");
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(Some(1..2), "y", window, cx)
    });
    assert_eq!(text(&view, cx), "ayc");
    cx.simulate_keystrokes("escape q");
    assert_eq!(register('a', cx).as_deref(), Some("Rx<BS>y\x1b"));

    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), "abc");
    cx.simulate_keystrokes("q b i");
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(Some(1..3), "Z", window, cx)
    });
    assert_eq!(text(&view, cx), "aZ");
    cx.simulate_keystrokes("escape q");
    assert_eq!(register('b', cx).as_deref(), Some("iZ\x1b"));
}

#[gpui::test]
fn explicit_range_commits_keep_their_range(cx: &mut TestAppContext) {
    // In replace mode, shorter text takes out the rest of the range, and
    // longer goes in before what it overwrites; `<BS>` puts back what was
    // overwritten. Empty text takes the range out.
    let cases: [(&str, Range<usize>, &str, &str, &str); 4] = [
        ("abcd", 0..2, "x", "xcd", "acd"),
        ("abcd", 0..1, "xyz", "xyzbcd", "xyabcd"),
        ("abcd", 1..3, "xy", "axyd", "axcd"),
        ("abcd", 0..2, "", "cd", "cd"),
    ];
    for (start, range, typed, after, back) in cases {
        let (view, cx) = editor(cx, start, 0, true);
        cx.simulate_keystrokes("q c shift-r");
        ime(&view, cx, |view, window, cx| {
            view.replace_text_in_range(Some(range.clone()), typed, window, cx)
        });
        assert_eq!(text(&view, cx), after, "{typed:?} over {range:?}");
        cx.simulate_keystrokes("backspace");
        assert_eq!(text(&view, cx), back, "{typed:?} over {range:?}");
        cx.simulate_keystrokes("escape q");
        let typed_keys = format!("R{typed}<BS>\x1b");
        assert_eq!(register('c', cx).as_deref(), Some(typed_keys.as_str()));
        cx.simulate_keystrokes("u");
        assert_eq!(text(&view, cx), start, "{typed:?} over {range:?} undone");
    }

    // Where typing wouldn't overwrite char for grapheme, the range is
    // replaced as given: over a line break, with one, a tab, or a mark that
    // joins the char before it.
    let cases: [(&str, Range<usize>, &str, &str); 8] = [
        ("ab\ncd", 0..3, "XYZ", "XYZcd"),
        ("abcd", 0..2, "x\n", "x\ncd"),
        ("abcd", 1..2, "x\r\ny", "ax\nycd"),
        ("abcd", 1..3, "\t", "a\td"),
        ("abcd", 0..1, "e\u{301}", "e\u{301}bcd"),
        // A range in UTF-16, as the input method gives it: `ñb`.
        ("añb", 1..3, "z", "az"),
        // A range inside a grapheme: nothing outside it is overwritten.
        ("a\u{301}b", 0..1, "x", "x\u{301}b"),
        ("ab\u{301}c", 0..2, "x", "x\u{301}c"),
    ];
    for (start, range, typed, after) in cases {
        let (view, cx) = editor(cx, start, 0, true);
        cx.simulate_keystrokes("q c shift-r");
        ime(&view, cx, |view, window, cx| {
            view.replace_text_in_range(Some(range.clone()), typed, window, cx)
        });
        assert_eq!(text(&view, cx), after, "{typed:?} over {range:?}");
        cx.simulate_keystrokes("escape q");
        assert_eq!(mode(&view, cx), Some("normal"));
        assert!(register('c', cx).is_some_and(|keys| keys.starts_with('R')));
        cx.simulate_keystrokes("u");
        assert_eq!(text(&view, cx), start, "{typed:?} over {range:?} undone");
    }
}

/// Composing in replace mode, the underline caret is at the input method's
/// caret: under the composition's grapheme after it, or past its end under
/// the buffer's grapheme the composition is before.
#[gpui::test]
fn replace_mode_composition_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "abcd", 0, true);
    cx.simulate_keystrokes("shift-r");
    let underline = |selected: Range<usize>, cx: &mut VisualTestContext| {
        ime(&view, cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "かな", Some(selected), window, cx)
        });
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(view.painted_carets.len(), 1);
            let caret = view.painted_carets[0];
            let layout = view.layout.as_mut().unwrap();
            let inlay = |range: Range<usize>| layout.inlay_bounds(range).unwrap();
            let (ka, na) = (inlay(0..3), inlay(3..6));
            let past = layout.caret_bounds(1).unwrap().left() - na.right();
            (caret, ka, na, past)
        })
    };
    let check = |caret: Bounds<Pixels>, left: Pixels, width: Pixels, bottom: Pixels| {
        assert_eq!(caret.origin.x, left);
        assert_eq!(caret.size.width, width);
        assert_eq!(caret.size.height, px(2.));
        assert_eq!(caret.bottom(), bottom);
    };
    let (caret, ka, _, _) = underline(0..0, cx);
    check(caret, ka.left(), ka.size.width, ka.bottom());
    let (caret, ka, na, _) = underline(1..1, cx);
    check(caret, ka.right(), na.size.width, na.bottom());
    // Past the composition, under `a`, which it's drawn before.
    let (caret, _, na, a_width) = underline(2..2, cx);
    assert!(a_width > px(2.));
    check(caret, na.right(), a_width, na.bottom());
}

/// Keys the keymap hands back to vim, from a chord that failed, timed out
/// or was handed over with `vim.key`, are refused as typed ones are.
#[gpui::test]
fn read_only_keymap_replays(cx: &mut TestAppContext) {
    use std::path::Path;

    use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};

    const TEXT: &str = "abc\ndef\n";
    let (view, cx) = editor(cx, TEXT, 0, true);
    let user = r#"
        [[bindings]]
        keys = "x z"
        command = "editor.select_all"
        context = "editor && vim.mode == normal"

        [[bindings]]
        keys = "x cmd-j"
        command = "vim.key"
        context = "editor && vim.mode == normal"
    "#;
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let (user, issues) = KeymapLayer::parse(Path::new("keymap.toml"), user, None);
    assert_eq!(issues, []);
    let keymap = Keymap::from_layers([default.unwrap(), user.unwrap()]);
    cx.update(|_, cx| init_with_keymap(cx, keymap));

    // Editable, a failed chord's `x` is replayed to vim, which deletes.
    cx.simulate_keystrokes("x j");
    assert_eq!(text(&view, cx), "bc\ndef\n");
    cx.simulate_keystrokes("u g g");
    assert_eq!(text(&view, cx), TEXT);

    set_read_only(&view, true, cx);
    let before = state(&view, cx);
    let refused = |what: &str, cx: &mut VisualTestContext| {
        assert_eq!(state(&view, cx), before, "{what}");
        assert_eq!(mode(&view, cx), Some("normal"), "{what}");
        assert_eq!(message(&view, cx).as_deref(), Some(E21), "{what}");
    };
    // Failed: `j` doesn't go on from `x`, which is replayed, as above. The
    // keymap then matches `j` afresh, which ends vim's message as any key
    // does, so only the text and the mode tell.
    cx.simulate_keystrokes("x j");
    assert_eq!(state(&view, cx), before);
    assert_eq!((mode(&view, cx), caret_row(&view, cx)), (Some("normal"), 1));
    // Timed out.
    cx.simulate_keystrokes("x");
    cx.executor().advance_clock(keymap::CHORD_TIMEOUT * 2);
    cx.run_until_parked();
    refused("timed out", cx);
    // Handed over: the chord is bound to `vim.key`, so `x` goes to vim, and
    // `cmd-j`, which vim has no key for, after it.
    cx.simulate_keystrokes("x cmd-j");
    refused("handed over", cx);
}

/// Text an input method commits with line breaks (`\r` alone and `\r\n`
/// too) or tabs in it goes in as its keys do, `Enter` copying the row's
/// indentation and `Tab` as the key types it, so a recording replays it
/// alike; and a `Tab` key records and replays.
#[gpui::test]
fn committed_breaks_and_tabs_replay_alike(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "  a\n  b\n", 0, true);
    let replays_alike = |keys: &str, input: &str, register: &str, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model_mut().set_cursor(0));
        let before = text(&view, cx);
        cx.simulate_keystrokes(&format!("q {register} {keys}"));
        // One commit, as an input method makes it.
        ime(&view, cx, |view, window, cx| {
            view.replace_text_in_range(None, input, window, cx)
        });
        cx.simulate_keystrokes("escape q");
        let typed = text(&view, cx);
        assert_ne!(typed, before, "{keys} {input:?}");
        cx.simulate_keystrokes("u");
        assert_eq!(text(&view, cx), before, "{keys} {input:?} undone");
        view.update(cx, |view, _| view.model_mut().set_cursor(0));
        cx.simulate_keystrokes(&format!("@ {register}"));
        assert_eq!(text(&view, cx), typed, "{keys} {input:?} replayed");
        assert_eq!(mode(&view, cx), Some("normal"), "{keys} {input:?} replayed");
        cx.simulate_keystrokes("u");
        typed
    };
    let typed = replays_alike("shift-a", "x\ny\rz\r\nt\tw", "a", cx);
    assert_eq!(typed, "  ax\n  y\n  z\n  t w\n  b\n");
    let typed = replays_alike("shift-r", "p\n\tq", "b", cx);
    assert_eq!(typed, "p\n    q\n  b\n");
    // The `Tab` key.
    let typed = replays_alike("shift-a tab", "v", "c", cx);
    assert_eq!(typed, "  a v\n  b\n");
}

/// A command line's submission is refused by what it would run: `:normal`
/// keys that undo, redo or put from the clipboard, alone or under `:g`,
/// with history to undo and redo and text on the clipboard. A visual
/// search after edits runs.
#[gpui::test]
fn read_only_command_lines(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "foo bar\nbaz foo\n", 0, true);
    // Something to undo and something to redo.
    cx.simulate_keystrokes("x x u");
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("CLIP".into())));
    set_read_only(&view, true, cx);
    let before = state(&view, cx);
    for line in [
        "normal u",
        "normal <C-r>",
        "normal \"+p",
        "normal \"*P",
        "normal jx",
        "g/foo/normal u",
        "g/foo/normal \"+p",
        "g/baz/s/a/b/",
    ] {
        submit(":", line, cx);
        assert_eq!(state(&view, cx), before, ":{line}");
        assert_eq!(mode(&view, cx), Some("normal"), ":{line}");
        assert_eq!(message(&view, cx).as_deref(), Some(E21), ":{line}");
    }
    // Yanks and motions run, under `:g` too.
    submit(":", "normal \"ayw", cx);
    assert_eq!(message(&view, cx), None);
    assert_eq!(register('a', cx).as_deref(), Some("oo "));
    submit(":", "g/baz/normal $", cx);
    assert_eq!((caret_row(&view, cx), message(&view, cx)), (1, None));

    // A visual search, with the visual anchor from text that has changed.
    cx.simulate_keystrokes("g g 0 v");
    submit("/", "foo", cx);
    assert_eq!(mode(&view, cx), Some("visual"));
    // The selection runs over the cursor's grapheme, on `foo` at 11.
    assert_eq!(head(&view, cx), 12);
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), before);
}

/// A text block's line of grapheme clusters, too long to shape whole, is
/// cut between clusters wherever it's scrolled to, through to its end;
/// with soft wrap on, the blocks' lines scroll and the wrapped rows don't.
#[gpui::test]
fn host_lines_scroll_by_cluster_and_under_soft_wrap(cx: &mut TestAppContext) {
    use fe_display::layout::MAX_SHAPED_ROW_BYTES;
    use unicode_segmentation::UnicodeSegmentation as _;

    const ASTRONAUT: &str = "👩\u{200d}🚀";
    let (view, cx) = editor(cx, &rows(5), 0, false);
    let clustered = format!("{}TAIL", ASTRONAUT.repeat(MAX_SHAPED_ROW_BYTES / 4));
    let wide = format!("{}END", "x".repeat(2_000));
    let lines = HostBlockKind::Lines {
        lines: vec![clustered.clone().into(), wide.into()],
        tint: Tint::Added,
        words: Vec::new(),
    };
    let block = HostBlock {
        above: 2,
        rows: 2,
        kind: lines,
        id: 5,
    };
    view.update(cx, |view, cx| view.set_blocks(vec![block], cx));
    let scroll_to = |x: f32, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            view.scroll.x = px(x);
            view.autoscroll = false;
            cx.notify();
        });
        let shown = painted_blocks(&view, cx);
        let lines: Vec<_> = shown[0]
            .lines
            .iter()
            .map(|(text, at)| (text.clone(), *at))
            .collect();
        let scrolled = view.read_with(cx, |view, _| view.scroll.x);
        (lines, f32::from(scrolled))
    };
    let mut last = 0.;
    let mut x = 0.;
    loop {
        let (lines, scrolled) = scroll_to(x, cx);
        let slice = &lines[0].0;
        assert!(slice.len() <= MAX_SHAPED_ROW_BYTES);
        assert!(clustered.contains(slice.as_str()));
        // Whole clusters only.
        let first = slice.graphemes(true).next().unwrap();
        assert!(first == ASTRONAUT || first == "T", "{first:?} at {x}");
        if scrolled == last && x > 0. {
            // As far as it goes: the line's end.
            assert!(slice.ends_with("TAIL"), "at {x}");
            break;
        }
        last = scrolled;
        x += 20_000.;
    }

    // Soft wrap on: the rows stay, the blocks' lines scroll.
    cx.simulate_keystrokes("alt-z");
    let at = on_row(&view, 0, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(-1e7), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    let shown = painted_blocks(&view, cx);
    let (frame_x, text_left, row_x) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let row = frame.row(0).unwrap();
        let row_x = layout.to_window(frame.line_origin(row)).x;
        (
            frame.scroll.x,
            layout.bounds(frame.text_bounds()).left(),
            row_x,
        )
    });
    assert_eq!(frame_x, 0.);
    assert_eq!(row_x, text_left);
    assert!(view.read_with(cx, |view, _| view.scroll.x) > px(0.));
    let (slice, origin) = &shown[0].lines[0];
    assert!(slice.ends_with("TAIL"));
    assert!(origin.x < text_left);
    let (wide, origin) = &shown[0].lines[1];
    assert!(wide.ends_with("END") && origin.x < text_left);
}
