//! `imap` and `inoremap` in the editor: a typed key asked of the maps
//! before it types ([`VimState::remap_insert`]), a held prefix owning the
//! next keystroke until [`CHORD_TIMEOUT`], and macros recording what was
//! typed, not what it expanded to.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, EntityInputHandler as _, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

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

fn inoremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Insert],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

fn waiting(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.vim_state().unwrap().remap_waiting())
}

fn time_out(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
}

/// A key typed as the window does: dispatched, and the text it leaves
/// typed by the input handler.
fn type_keys(cx: &mut VisualTestContext, keys: &str) {
    for key in keys.chars() {
        cx.simulate_keystrokes(&key.to_string());
    }
}

#[gpui::test]
fn jk_leaves_insert_and_other_keys_type(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("jk", "<Esc>")]);
    cx.simulate_keystrokes("i j");
    assert_eq!(text(&view, cx), "");
    assert!(waiting(&view, cx));
    cx.simulate_keystrokes("k");
    assert_eq!(text(&view, cx), "");
    assert_eq!(mode(&view, cx), Some("normal"));

    cx.simulate_keystrokes("i j x");
    assert_eq!(text(&view, cx), "jx");
    assert_eq!(mode(&view, cx), Some("insert"));
    assert!(!waiting(&view, cx));
}

#[gpui::test]
fn held_prefix_types_on_timeout_blur_or_press(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("jk", "X")]);
    cx.simulate_keystrokes("i j");
    assert_eq!(text(&view, cx), "");
    assert!(waiting(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(view.vim_state().unwrap().pending().kind().is_none())
    });
    time_out(cx);
    assert_eq!(text(&view, cx), "j");
    assert!(!waiting(&view, cx));

    cx.simulate_keystrokes("j");
    assert!(waiting(&view, cx));
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(text(&view, cx), "jj");
    assert!(!waiting(&view, cx));

    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_keystrokes("j");
    assert!(waiting(&view, cx));
    // Typed through, the map runs.
    cx.simulate_keystrokes("k");
    assert_eq!(text(&view, cx), "jjX");
}

#[gpui::test]
fn replace_mode_overwrites_with_the_expansion(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abcdef", vec![inoremap("jk", "XY")]);
    cx.simulate_keystrokes("shift-r j k");
    assert_eq!(text(&view, cx), "XYcdef");
    assert_eq!(mode(&view, cx), Some("replace"));
    type_keys(cx, "j");
    time_out(cx);
    assert_eq!(text(&view, cx), "XYjdef");
}

#[gpui::test]
fn commits_and_compositions_are_not_matched(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("jk", "X"), inoremap("j", "Y")]);
    cx.simulate_keystrokes("i");
    // A paste or an IME result is a commit, not keys.
    view.update_in(cx, |view, window, cx| {
        view.replace_text_in_range(None, "jk", window, cx)
    });
    assert_eq!(text(&view, cx), "jk");
    assert!(!waiting(&view, cx));
    // A composition in progress is never matched.
    view.update_in(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "j", None, window, cx)
    });
    cx.simulate_keystrokes("k");
    assert!(!waiting(&view, cx));
    assert!(!text(&view, cx).contains('X'));
    assert!(!text(&view, cx).contains('Y'));
}

#[gpui::test]
fn macros_record_typed_keys_and_replay_the_map(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(
        cx,
        "",
        vec![inoremap("jk", "<Esc>"), inoremap("zz", "A<CR>B")],
    );
    cx.simulate_keystrokes("q a i a j k q");
    assert_eq!(text(&view, cx), "a");
    assert_eq!(mode(&view, cx), Some("normal"));
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "aa");
    assert_eq!(mode(&view, cx), Some("normal"));

    cx.simulate_keystrokes("shift-g shift-a z z");
    assert_eq!(text(&view, cx), "aaA\nB");
    assert_eq!(mode(&view, cx), Some("insert"));
}

#[gpui::test]
fn the_keymap_wins_over_an_insert_map(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("<Esc>", "X")]);
    cx.simulate_keystrokes("i");
    // `escape` is bound in insert mode: it's the keymap's, never offered.
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&view, cx), "");
    assert_eq!(mode(&view, cx), Some("normal"));
}

/// The key that breaks a held prefix is matched on its own: `x` after the
/// `j` of `jk` is `inoremap x Y`'s.
#[gpui::test]
fn the_key_that_breaks_a_prefix_starts_another_map(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("jk", "X"), inoremap("x", "Y")]);
    cx.simulate_keystrokes("i j x");
    assert_eq!(text(&view, cx), "jY");
    assert!(!waiting(&view, cx));
}

/// A direct commit or a composition while keys are held resolves them
/// first: they type before it, and the composition never completes a map.
#[gpui::test]
fn ime_input_resolves_a_held_prefix(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("jk", "X")]);
    cx.simulate_keystrokes("i j");
    assert!(waiting(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.replace_text_in_range(None, "abc", window, cx)
    });
    assert_eq!(text(&view, cx), "jabc");
    assert!(!waiting(&view, cx));
    time_out(cx);
    assert_eq!(text(&view, cx), "jabc");

    cx.simulate_keystrokes("j");
    assert!(waiting(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "z", None, window, cx)
    });
    assert!(!waiting(&view, cx));
    cx.simulate_keystrokes("k");
    assert!(!text(&view, cx).contains('X'));
}

/// Held keys that resolve to `<Esc>` leave insert mode, where an input
/// method's text isn't typed.
#[gpui::test]
fn ime_input_after_a_prefix_that_leaves_insert_is_dropped(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "", vec![inoremap("j", "<Esc>"), inoremap("jk", "X")]);
    cx.simulate_keystrokes("i j");
    assert!(waiting(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.replace_text_in_range(None, "abc", window, cx)
    });
    assert_eq!(text(&view, cx), "");
    assert_eq!(mode(&view, cx), Some("normal"));

    cx.simulate_keystrokes("i j");
    assert!(waiting(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "z", None, window, cx)
    });
    assert_eq!(text(&view, cx), "");
    assert_eq!(mode(&view, cx), Some("normal"));
    view.read_with(cx, |view, _| assert!(view.composition().is_none()));
}
