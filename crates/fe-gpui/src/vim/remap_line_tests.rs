//! `cnoremap` on the editor's side of the command line: the keys the host's
//! line asks it about ([`EditorView::remap_line_key`]), a macro recording
//! what was typed, and a replay's line matching the `c` maps.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, commands::RunCommand, init};

fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    remaps: Vec<Remap>,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn cnoremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::CmdLine],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn register(name: char, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        let registers = &cx.global::<Vim>().registers;
        registers.get(name).map(|register| register.text.clone())
    })
}

/// What a host's line does with `typed`, character by character: the keys
/// the maps give go in, and the line ends on a `<CR>`. Returns the line.
fn type_line(view: &Entity<EditorView>, typed: &str, cx: &mut VisualTestContext) -> String {
    let mut line = String::new();
    for ch in typed.chars() {
        let keys = view.update(cx, |view, _| view.remap_line_key(ch));
        let Some(keys) = keys else {
            line.push(ch);
            continue;
        };
        for (key, _) in keys.typed {
            match key {
                VimKey::Char(ch) => line.push(ch),
                VimKey::Enter => {
                    submit(view, &line, cx);
                    return line;
                }
                key => panic!("unexpected {key:?}"),
            }
        }
    }
    line
}

fn submit(view: &Entity<EditorView>, line: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| view.record_command_line(line, VimKey::Enter));
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": line }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(submit)), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn without_maps_a_line_key_is_left_to_the_input(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![]);
    cx.simulate_keystrokes(":");
    assert_eq!(view.update(cx, |view, _| view.remap_line_key('j')), None);
}

#[gpui::test]
fn a_held_prefix_gives_nothing_until_it_resolves(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    assert_eq!(
        view.update(cx, |view, _| view.remap_line_key('j')),
        Some(LineKeys::default())
    );
    assert!(view.read_with(cx, |view, _| view.remap_line_waiting()));
    let keys = view.update(cx, |view, _| view.remap_line_key('x'));
    assert_eq!(
        keys.map(|keys| keys.typed),
        Some(vec![(VimKey::Char('j'), false), (VimKey::Char('x'), false)])
    );
    // And a timeout gives the held key alone.
    view.update(cx, |view, _| view.remap_line_key('j'));
    assert_eq!(
        view.update(cx, |view, _| view.remap_line_timeout().typed),
        vec![(VimKey::Char('j'), false)]
    );
    assert!(!view.read_with(cx, |view, _| view.remap_line_waiting()));
}

#[gpui::test]
fn a_map_to_enter_ends_the_line_with_an_expanded_key(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    view.update(cx, |view, _| view.remap_line_key('j'));
    let keys = view.update(cx, |view, _| view.remap_line_key('k'));
    assert_eq!(
        keys.map(|keys| keys.typed),
        Some(vec![(VimKey::Enter, true)])
    );
}

#[gpui::test]
fn a_recording_has_the_line_as_typed_and_replays_it(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    let line = type_line(&view, "s/a/b/jk", cx);
    assert_eq!(line, "s/a/b/");
    cx.simulate_keystrokes("q");
    assert_eq!(text(&view, cx), "baa");
    // Once, as typed: no <CR> of the map's, no second copy of the line.
    assert_eq!(register('a', cx).as_deref(), Some(":s/a/b/jk"));

    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "bba");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
}

#[gpui::test]
fn a_replayed_line_expands_a_map_that_types_text(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("zz", "s/a/b/<CR>")]);
    cx.simulate_keystrokes("q a :");
    let keys = view.update(cx, |view, _| view.remap_line_key('z'));
    assert_eq!(keys, Some(LineKeys::default()));
    let keys = view.update(cx, |view, _| view.remap_line_key('z'));
    let line: String = keys
        .unwrap()
        .typed
        .iter()
        .filter_map(|(key, _)| match key {
            VimKey::Char(ch) => Some(*ch),
            _ => None,
        })
        .collect();
    assert_eq!(line, "s/a/b/");
    submit(&view, &line, cx);
    cx.simulate_keystrokes("q");
    assert_eq!(register('a', cx).as_deref(), Some(":zz"));
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "bba");
}

#[gpui::test]
fn keys_after_a_map_s_enter_are_left_for_the_editor(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<CR>x")]);
    cx.simulate_keystrokes(":");
    view.update(cx, |view, _| view.remap_line_key('j'));
    let keys = view.update(cx, |view, _| view.remap_line_key('k')).unwrap();
    assert_eq!(keys.typed, vec![(VimKey::Enter, true)]);
    assert_eq!(keys.rest, vec![VimKey::Char('x')]);
}

#[gpui::test]
fn a_replayed_map_s_escape_cancels_the_line(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<Esc>")]);
    cx.simulate_keystrokes("q a :");
    view.update(cx, |view, _| view.remap_line_key('j'));
    view.update(cx, |view, _| view.remap_line_key('k'));
    view.update(cx, |view, _| view.record_command_line("", VimKey::Esc));
    let cancel = Invocation::of("vim.command_line_cancel").unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(cancel)), cx));
    cx.simulate_keystrokes("q");
    assert_eq!(register('a', cx).as_deref(), Some(":jk"));
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "aaa");
}

#[gpui::test]
fn edits_the_maps_never_saw_are_recorded(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    for ch in "s/a/b/x".chars() {
        view.update(cx, |view, _| view.remap_line_key(ch));
    }
    // The line's own Backspace took the `x` out; Enter ends it.
    submit(&view, "s/a/b/", cx);
    cx.simulate_keystrokes("q");
    assert_eq!(register('a', cx).as_deref(), Some(":s/a/b/x<BS>\r"));
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "bba");
}

#[gpui::test]
fn a_new_line_forgets_the_last_ones_keys(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "aaa", vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    view.update(cx, |view, _| view.remap_line_key('a'));
    view.update(cx, |view, _| view.begin_command_line());
    cx.simulate_keystrokes("escape q a :");
    view.update(cx, |view, _| view.record_command_line("zz", VimKey::Enter));
    cx.simulate_keystrokes("escape q");
    assert_eq!(register('a', cx).as_deref(), Some(":zz\r\u{1b}"));
}
