//! The status bar's vim mode item is a segment filled in its mode's colour
//! from the editor's theme, its label in `vim_mode_foreground`.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, Hsla, SharedString, TestAppContext, VisualTestContext};
use kiln_shell::StatusFill;

use super::{status, status_items};
use crate::{EditorTheme, EditorView, set_vim_enabled, set_vim_jump_on_s};

/// The vim mode item's label and fill, if there's one.
fn mode_item(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Option<(SharedString, Option<StatusFill>)> {
    view.read_with(cx, |view, _| {
        status_items(view)
            .into_iter()
            .find(|item| item.id == status::VIM_MODE)
            .map(|item| (item.text, item.fill))
    })
}

#[gpui::test]
fn fills_each_mode(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("one two\nthree\n"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    assert_eq!(mode_item(&view, cx), None, "no mode item with vim off");

    cx.update(|_, cx| {
        set_vim_enabled(cx, true);
        set_vim_jump_on_s(cx, true);
    });
    cx.run_until_parked();
    let theme = view.read_with(cx, |view, _| view.theme().clone());
    let fill = |background: Hsla| {
        Some(StatusFill {
            background,
            foreground: theme.vim_mode_foreground,
        })
    };
    let mut check = |keys: &str, label: &str, background: Hsla, leave: &str| {
        if !keys.is_empty() {
            cx.simulate_keystrokes(keys);
        }
        assert_eq!(
            mode_item(&view, cx),
            Some((label.into(), fill(background))),
            "after {keys:?}"
        );
        if !leave.is_empty() {
            cx.simulate_keystrokes(leave);
        }
    };
    check("", "NORMAL", theme.vim_mode_normal, "");
    check("d", "NORMAL", theme.vim_mode_normal, "escape");
    check("s", "JUMP", theme.vim_mode_normal, "escape");
    check("i", "INSERT", theme.vim_mode_insert, "escape");
    check("v", "VISUAL", theme.vim_mode_visual, "escape");
    check("shift-v", "VISUAL LINE", theme.vim_mode_visual, "escape");
    check("ctrl-v", "VISUAL BLOCK", theme.vim_mode_visual, "escape");
    check("shift-r", "REPLACE", theme.vim_mode_replace, "escape");
    // A jump from a visual mode is normal mode's colour while it's pending,
    // and the visual mode's again once it's cancelled.
    for (keys, label) in [
        ("v", "VISUAL"),
        ("shift-v", "VISUAL LINE"),
        ("ctrl-v", "VISUAL BLOCK"),
    ] {
        check(keys, label, theme.vim_mode_visual, "");
        check("s", "JUMP", theme.vim_mode_normal, "");
        check("t", "JUMP t", theme.vim_mode_normal, "escape");
        check("", label, theme.vim_mode_visual, "escape");
    }
    // One completed from visual mode leaves it visual.
    check("v", "VISUAL", theme.vim_mode_visual, "");
    check("g s", "JUMP", theme.vim_mode_normal, "t h");
    check("", "VISUAL", theme.vim_mode_visual, "escape");
    check("", "NORMAL", theme.vim_mode_normal, "");

    let colors = [
        theme.vim_mode_normal,
        theme.vim_mode_insert,
        theme.vim_mode_visual,
        theme.vim_mode_replace,
    ];
    for (i, a) in colors.iter().enumerate() {
        assert!(colors[i + 1..].iter().all(|b| a != b), "modes share a fill");
    }

    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    assert_eq!(mode_item(&view, cx), None, "no mode item with vim off");
}
