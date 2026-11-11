//! The which-key popup for a prefix the keymap matcher holds for a chord
//! (`]` of `] d`, `g` of `g b`, `ctrl-w`), with the default keymap: after
//! the delay it lists the chord's next keys with their commands' titles,
//! merged with vim's own keys after the same prefix, never for a chord
//! typed faster, and the chord ending closes it.
//!
//! Only an editor's chords show: one held elsewhere, such as `cmd-k` in a
//! panel, shows nothing, there being no popup there.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::Buffer;
use fe_command::{CommandId, CommandMeta};
use fe_editor::EditorModel;
use fe_vim::{WhichKey, WhichKeyLabel};
use gpui::{
    Entity, Focusable as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point,
};

use super::*;
use crate::{EditorTheme, EditorView, init, keymap::chord_pending};

const DELAY: Duration = Duration::from_millis(200);

/// The workbench's commands the chords run, with their titles, as the host
/// hands them over ([`set_host_commands`]): fe-gpui has none of them.
const HOST_COMMANDS: &[(&str, &str)] = &[
    ("diagnostics.next", "Go to Next Problem"),
    ("diagnostics.previous", "Go to Previous Problem"),
    ("nav.goto_definition", "Go to Definition"),
    ("pane.focus_left", "Focus Left Pane"),
    ("pane.focus_right", "Focus Right Pane"),
    ("pane.focus_up", "Focus Pane Above"),
    ("pane.focus_down", "Focus Pane Below"),
    ("pane.focus_next", "Focus Next Pane"),
    ("pane.focus_previous", "Focus Previous Pane"),
    ("pane.split_down", "Split Down"),
    ("pane.split_right", "Split Right"),
    ("pane.close", "Close Pane"),
];

/// A focused editor showing `text` in a window of its own, with the
/// default keymap, vim on, the popup after [`DELAY`], and the host's
/// commands handed over.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        set_vim_enabled(cx, true);
        set_vim_which_key(cx, true, DELAY);
    });
    let host = (HOST_COMMANDS.iter())
        .map(|&(id, title)| CommandMeta {
            id: CommandId::parse(id).unwrap(),
            title: title.into(),
            description: String::new(),
            args_schema: None,
            context: None,
            mutates_buffer: false,
        })
        .collect();
    set_host_commands(host);
    cx.run_until_parked();
    (view, cx)
}

fn popup(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<WhichKey> {
    view.read_with(cx, |view, _| view.which_key_shown().cloned())
}

fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

/// Each key listed, with its title as the popup shows it.
fn titles(which: &WhichKey, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    cx.update(|_, cx| {
        (which.entries.iter())
            .map(|entry| (entry.key.clone(), which_key::title(entry, cx)))
            .collect()
    })
}

fn title_of(which: &WhichKey, key: &str, cx: &mut VisualTestContext) -> String {
    let titles = titles(which, cx);
    (titles.into_iter().find(|(listed, _)| listed == key))
        .unwrap_or_else(|| panic!("{key} not listed in {which:?}"))
        .1
}

fn editor_title(id: &str) -> String {
    editor_command(&CommandId::parse(id).unwrap())
        .unwrap_or_else(|| panic!("the editor's {id}"))
        .title
}

/// The caret's row and column.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = (model.text()).offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

/// Records the commands dispatched as a `RunCommand` that nothing takes,
/// as the workbench's are here.
fn record_commands(cx: &mut VisualTestContext) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        cx.on_action(move |action: &crate::commands::RunCommand, _| {
            sink.borrow_mut().push(action.0.command.as_str().to_owned());
        });
    });
    seen
}

/// `]` held shows, after the delay, `d` as `diagnostics.next` by its title
/// with vim's own `]` keys merged; `[` shows `d` as `diagnostics.previous`;
/// `g` shows the keymap's `b`, `d` and the `r` group with the built-in `g`
/// keys of the reducer. A chord typed faster than the delay never shows
/// it.
#[gpui::test]
fn bracket_and_g_chords_list_their_keys(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one two\nthree four\nfive six");
    cx.simulate_keystrokes("]");
    assert!(cx.update(|_, cx| chord_pending(cx)), "the matcher holds ]");
    assert_eq!(popup(&view, cx), None);
    wait(DELAY - Duration::from_millis(1), cx);
    assert_eq!(popup(&view, cx), None);
    wait(Duration::from_millis(2), cx);
    let which = popup(&view, cx).expect("]'s popup after the delay");
    assert_eq!(which.prefix, "]");
    assert_eq!(title_of(&which, "d", cx), "Go to Next Problem");
    assert_eq!(title_of(&which, "c", cx), "next change");
    assert_eq!(title_of(&which, "}", cx), "next unmatched }");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    assert_eq!(popup(&view, cx), None);

    cx.simulate_keystrokes("[");
    wait(DELAY, cx);
    let which = popup(&view, cx).expect("['s popup");
    assert_eq!(which.prefix, "[");
    assert_eq!(title_of(&which, "d", cx), "Go to Previous Problem");
    assert_eq!(title_of(&which, "c", cx), "previous change");
    cx.simulate_keystrokes("escape");

    cx.simulate_keystrokes("g");
    wait(DELAY, cx);
    let which = popup(&view, cx).expect("g's popup");
    assert_eq!(which.prefix, "g");
    assert_eq!(
        title_of(&which, "b", cx),
        editor_title("editor.add_caret_at_next_occurrence")
    );
    assert_eq!(title_of(&which, "d", cx), "Go to Definition");
    // `grr`, `grn`, `gra`, `gri` and `grt`, with ReplaceWithRegister off.
    let r = (which.entries.iter())
        .find(|entry| entry.key == "r")
        .unwrap();
    assert_eq!(
        (&r.label, r.group),
        (
            &WhichKeyLabel::Group {
                count: 5,
                name: None
            },
            true
        )
    );
    // The reducer's own `g` keys, of T1134's table.
    assert_eq!(title_of(&which, "g", cx), "first line");
    assert_eq!(title_of(&which, "e", cx), "end of previous word");
    assert_eq!(title_of(&which, "j", cx), "down a display line");
    // Sorted as vim's keys are, a chord's among them.
    let keys: Vec<_> = (which.entries.iter())
        .map(|entry| entry.key.as_str())
        .collect();
    let at = |key| keys.iter().position(|listed| *listed == key).unwrap();
    assert!(
        at("b") < at("c") && at("c") < at("d") && at("d") < at("e"),
        "{keys:?}"
    );
    cx.simulate_keystrokes("escape");

    // Faster than the delay: `] d` runs, `g b` adds a caret, `g g` goes to
    // the first line; no popup shows.
    let seen = record_commands(cx);
    cx.simulate_keystrokes("j ] d");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
    assert!(
        seen.borrow().iter().any(|id| id == "diagnostics.next"),
        "{seen:?}"
    );
    cx.simulate_keystrokes("g b");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
    cx.simulate_keystrokes("escape g g");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
    assert_eq!(caret(&view, cx), (0, 0));
}

/// `ctrl-w` held shows the window keys: focusing a pane each way, the next
/// and previous, splitting and closing, each with its command's title.
#[gpui::test]
fn ctrl_w_lists_window_commands(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one\ntwo");
    cx.simulate_keystrokes("ctrl-w");
    assert_eq!(popup(&view, cx), None);
    wait(DELAY, cx);
    let which = popup(&view, cx).expect("ctrl-w's popup");
    assert_eq!(which.prefix, "<C-w>");
    let expected = [
        ("h", "Focus Left Pane"),
        ("<C-h>", "Focus Left Pane"),
        ("<Left>", "Focus Left Pane"),
        ("j", "Focus Pane Below"),
        ("k", "Focus Pane Above"),
        ("l", "Focus Right Pane"),
        ("<Right>", "Focus Right Pane"),
        ("w", "Focus Next Pane"),
        ("<C-w>", "Focus Next Pane"),
        ("W", "Focus Previous Pane"),
        ("s", "Split Down"),
        ("S", "Split Down"),
        ("<C-s>", "Split Down"),
        ("v", "Split Right"),
        ("<C-v>", "Split Right"),
        ("c", "Close Pane"),
        ("q", "Close Pane"),
        ("<C-q>", "Close Pane"),
    ];
    for (key, title) in expected {
        assert_eq!(title_of(&which, key, cx), title, "{key}");
    }
    // Vim takes no key after `<C-w>`: every one is the keymap's.
    assert!(
        (which.entries.iter()).all(|entry| matches!(entry.label, WhichKeyLabel::Command(_))),
        "{which:?}"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // The key runs its command and closes the popup.
    let seen = record_commands(cx);
    cx.simulate_keystrokes("v");
    assert_eq!(popup(&view, cx), None);
    assert_eq!(*seen.borrow(), ["pane.split_right"]);
}

/// A chord completed, a failed one, [`CHORD_TIMEOUT`], `<Esc>`, a keymap
/// reload and a blur each close the popup; the keys the matcher replays to vim reach it as
/// typed, and a prefix they leave waiting there waits anew, for vim's own
/// popup.
#[gpui::test]
fn chord_end_closes_the_popup(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one two\nthree four\nfive six");
    let seen = record_commands(cx);
    // Holds `keys` past the delay: the popup shows `prefix`'s keys.
    let shown = |keys: &str, prefix: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        wait(DELAY, cx);
        let which = popup(&view, cx).unwrap_or_else(|| panic!("{keys}'s popup"));
        assert_eq!(which.prefix, prefix);
    };

    // Completed: `] d` runs.
    shown("]", "]", cx);
    cx.simulate_keystrokes("d");
    assert_eq!(popup(&view, cx), None);
    assert_eq!(*seen.borrow(), ["diagnostics.next"]);
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);

    // Failed: `g` then `e` and `g` again are vim's, and reach it.
    cx.simulate_keystrokes("j w");
    assert_eq!(caret(&view, cx), (1, 6));
    shown("g", "g", cx);
    cx.simulate_keystrokes("e");
    assert_eq!(popup(&view, cx), None);
    assert_eq!(caret(&view, cx), (1, 4));
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
    shown("g", "g", cx);
    cx.simulate_keystrokes("g");
    assert_eq!(popup(&view, cx), None);
    // `gg`, keeping the column.
    assert_eq!(caret(&view, cx), (0, 4));
    assert!(!cx.update(|_, cx| chord_pending(cx)));

    // A popup shown holds the chord past the timeout (see the
    // `which_key_hold` tests); with none shown it times out as ever: `g`
    // goes to vim, which waits on it.
    cx.simulate_keystrokes("j");
    cx.update(|_, cx| set_vim_which_key(cx, true, CHORD_TIMEOUT * 2));
    cx.simulate_keystrokes("g");
    assert!(cx.update(|_, cx| chord_pending(cx)));
    wait(CHORD_TIMEOUT + Duration::from_millis(1), cx);
    assert_eq!(popup(&view, cx), None);
    assert!(!cx.update(|_, cx| chord_pending(cx)));
    let pending = view.read_with(cx, |view, _| view.vim_state().unwrap().pending().kind());
    assert!(pending.is_some(), "vim waits on g");
    cx.simulate_keystrokes("g");
    assert_eq!(caret(&view, cx), (0, 4));
    cx.update(|_, cx| set_vim_which_key(cx, true, DELAY));

    // `<Esc>`: the chord fails, and vim drops the `]` it's handed.
    shown("]", "]", cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(popup(&view, cx), None);
    let pending = view.read_with(cx, |view, _| view.vim_state().unwrap().pending().kind());
    assert_eq!(pending, None);
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);

    // A keymap reload drops the chord.
    shown("]", "]", cx);
    cx.cx
        .update(|cx| crate::keymap::set_user_keymap(cx, crate::keymap::default_keymap()));
    cx.run_until_parked();
    assert_eq!(popup(&view, cx), None);
    assert!(!cx.update(|_, cx| chord_pending(cx)));

    // A blur abandons the chord.
    shown("ctrl-w", "<C-w>", cx);
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(popup(&view, cx), None);
    assert!(!cx.update(|_, cx| chord_pending(cx)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
}

/// The wheel over the popup scrolls its keys to the hidden ones, never the
/// text under it (T225 F6).
#[gpui::test]
fn the_wheel_scrolls_the_popup_not_the_text(cx: &mut TestAppContext) {
    let text: Vec<String> = (0..200).map(|row| format!("row {row}")).collect();
    let (view, cx) = vim_editor(cx, &text.join("\n"));
    cx.simulate_resize(gpui::size(gpui::px(320.), gpui::px(240.)));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-w");
    wait(DELAY, cx);
    assert!(popup(&view, cx).is_some());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let entries = cx
        .debug_bounds("which-key-entries")
        .expect("its keys paint");
    // `z`, sorted after `w`, `W` and `x`, is the last key.
    let which = popup(&view, cx).unwrap();
    assert_eq!(which.entries.last().unwrap().key, "z");
    let last = |cx: &mut VisualTestContext| {
        cx.debug_bounds("which-key-entry-z")
            .expect("the last key lays out")
    };
    let before = last(cx);
    assert!(before.top() > entries.bottom(), "the last key is hidden");
    let scroll = view.read_with(cx, |view, _| view.scroll);

    cx.simulate_event(ScrollWheelEvent {
        position: entries.center(),
        delta: ScrollDelta::Lines(point(0., -100.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let after = last(cx);
    assert!(after.top() < before.top(), "{after:?} from {before:?}");
    assert!(
        after.bottom() <= entries.bottom() + gpui::px(1.),
        "{after:?} in {entries:?}"
    );
    assert_eq!(view.read_with(cx, |view, _| view.scroll), scroll);
    assert!(popup(&view, cx).is_some());
}
