//! A press resolves keys held for a longer remap before it acts, and lands
//! on the text it was over: the held keys can edit the text.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode};
use gpui::{
    Entity, EntityInputHandler as _, Focusable as _, Modifiers, MouseButton, MouseDownEvent,
    Pixels, TestAppContext, VisualTestContext,
};

use crate::{
    EditorTheme, EditorView, init,
    vim::{set_vim_enabled, set_vim_remaps},
};

/// A focused vim editor showing `text`, the caret at the start, with
/// `xy` mapped so a lone `x` is held and resolves to a deletion.
fn held_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    held_editor_with(cx, text, None)
}

/// [`held_editor`] with a lone `x` mapped to `to`, if given.
fn held_editor_with<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    to: Option<&str>,
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
    let map = |from: &str, to: &str| Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    };
    let mut remaps = vec![map("xy", "l")];
    remaps.extend(to.map(|to| map("x", to)));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> std::ops::Range<usize> {
    view.read_with(cx, |view, _| view.model().primary_selection().range)
}

fn position_of(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> gpui::Point<Pixels> {
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        view.update(cx, |view, cx| {
            let utf16 = view.model().text().offset_to_utf16(offset);
            view.bounds_for_range(utf16..utf16, Default::default(), window, cx)
                .expect("visible")
                .center()
        })
    })
}

fn click(
    cx: &mut VisualTestContext,
    position: gpui::Point<Pixels>,
    modifiers: Modifiers,
    count: usize,
) {
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers,
        click_count: count,
        first_mouse: false,
    });
    // A press that waited for the held prefix runs a frame or two on.
    for _ in 0..6 {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(position, MouseButton::Left, modifiers);
}

/// The held `x` resolves to `x` (deleting the first character) before the
/// double click's word is found, at the offset the screen position has by
/// then.
#[gpui::test]
fn a_double_click_resolves_a_held_prefix_first(cx: &mut TestAppContext) {
    let (view, cx) = held_editor(cx, "aa bb");
    let at = position_of(&view, 4, cx);
    cx.simulate_keystrokes("x");
    assert_eq!(text(&view, cx), "aa bb");
    click(cx, at, Modifiers::none(), 2);
    assert_eq!(text(&view, cx), "a bb");
    assert_eq!(selection(&view, cx), 2..4);
    let waiting = view.read_with(cx, |view, _| view.vim_state().unwrap().remap_waiting());
    assert!(!waiting);
}

#[gpui::test]
fn a_triple_click_resolves_a_held_prefix_first(cx: &mut TestAppContext) {
    let (view, cx) = held_editor(cx, "aa bb\ncc");
    let at = position_of(&view, 7, cx);
    cx.simulate_keystrokes("x");
    click(cx, at, Modifiers::none(), 3);
    assert_eq!(text(&view, cx), "a bb\ncc");
    assert_eq!(selection(&view, cx).start, 5);
}

#[gpui::test]
fn a_shift_click_resolves_a_held_prefix_first(cx: &mut TestAppContext) {
    let (view, cx) = held_editor(cx, "aa bb");
    let at = position_of(&view, 3, cx);
    cx.simulate_keystrokes("x");
    click(cx, at, Modifiers::shift(), 1);
    assert_eq!(text(&view, cx), "a bb");
    assert_eq!(selection(&view, cx).end, 4);
}

/// A macro replay ends keyword completion with `<C-e>` or `<C-y>` as the
/// live keys did, and the keys after it still run.
#[gpui::test]
fn a_replay_cancels_and_accepts_keyword_completion(cx: &mut TestAppContext) {
    let (view, cx) = held_editor(cx, "foobar yyzz\nfo\nfo\nyy");
    let to = |view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let offset = view
                .model()
                .text()
                .point_to_offset(fe_buffer::Point::new(row, 0));
            view.model_mut().set_cursor(offset);
        });
    };
    to(&view, 1, cx);
    cx.simulate_keystrokes("q a shift-a ctrl-n ctrl-e x escape q");
    assert_eq!(text(&view, cx), "foobar yyzz\nfox\nfo\nyy");
    to(&view, 2, cx);
    cx.simulate_keystrokes("@ a");
    assert_eq!(text(&view, cx), "foobar yyzz\nfox\nfox\nyy");

    to(&view, 3, cx);
    cx.simulate_keystrokes("q b shift-a ctrl-n ctrl-y x escape q");
    assert_eq!(text(&view, cx), "foobar yyzz\nfox\nfox\nyyzzx");
    cx.simulate_keystrokes("shift-g o y y escape @ b");
    // The word taken may be either `yyzz` or the first row's, as nvim's
    // scan finds them; the key after `<C-y>` ran either way.
    let last = text(&view, cx).lines().last().unwrap().to_owned();
    assert!(last.starts_with("yyzz") && last.ends_with('x'), "{last:?}");
}

/// A resolution that deletes a line moves the rows under the pointer: the
/// press lands on the text it was over.
#[gpui::test]
fn a_press_lands_on_its_text_after_a_line_goes(cx: &mut TestAppContext) {
    let (view, cx) = held_editor_with(cx, "a\nbbbb\ncccc", Some("dd"));
    let at = position_of(&view, 4, cx);
    cx.simulate_keystrokes("x");
    click(cx, at, Modifiers::none(), 2);
    assert_eq!(text(&view, cx), "bbbb\ncccc");
    assert_eq!(selection(&view, cx), 5..9);
}

/// A map's `<Cmd>fe:…<CR>` runs once the keystroke is over: the press
/// waits for the keys after it, which act at the caret they started at.
#[gpui::test]
fn a_press_waits_for_a_held_maps_command(cx: &mut TestAppContext) {
    let (view, cx) = held_editor_with(cx, "aa bb", Some("<Cmd>fe:test.ping<CR>x"));
    cx.update(|_, cx| {
        kiln_platform::commands::register_app_commands(cx, |registry| {
            registry
                .register(
                    "test.ping",
                    "test.ping",
                    |_: &mut &mut gpui::App, _: &fe_command::Actor| {
                        Ok(fe_command::Dispatched::new(fe_command::Effect::Nothing))
                    },
                )
                .unwrap();
        })
    });
    let at = position_of(&view, 4, cx);
    cx.simulate_keystrokes("x");
    click(cx, at, Modifiers::none(), 2);
    cx.run_until_parked();
    assert_eq!(text(&view, cx), "a bb");
    assert_eq!(selection(&view, cx), 2..4);
}

/// A middle click resolves a held prefix too, then pastes where the pointer
/// is on the text as it stands.
#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
#[gpui::test]
fn a_middle_click_resolves_a_held_prefix_first(cx: &mut TestAppContext) {
    let (view, cx) = held_editor_with(cx, "a\nbbbb\ncccc", Some("dd"));
    view.update(cx, |view, _| view.set_primary_selection(true));
    cx.update(|_, cx| cx.write_to_primary(gpui::ClipboardItem::new_string("Z".into())));
    let at = position_of(&view, 4, cx);
    cx.simulate_keystrokes("x");
    cx.simulate_mouse_down(at, MouseButton::Middle, Modifiers::none());
    for _ in 0..6 {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(at, MouseButton::Middle, Modifiers::none());
    assert_eq!(text(&view, cx), "bbbb\nccZcc");
}
