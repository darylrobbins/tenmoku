//! Remaps, from the vimrc and `vim.remap`, reaching the editors' vim state.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_vim::{Lookup, Remap, RemapMode, VimKey};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

fn open(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new("text"), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

fn remap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

/// Whether the editor's state maps `H` in normal mode.
fn maps_h(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| {
        let keys = [VimKey::Char('H')];
        !matches!(
            view.vim_state()
                .unwrap()
                .remaps()
                .lookup(RemapMode::Normal, &keys),
            Lookup::None
        )
    })
}

#[gpui::test]
fn remaps_reach_every_editor(cx: &mut TestAppContext) {
    cx.update(init);
    let (first, cx) = open(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    assert!(!maps_h(&first, cx));

    let issues = cx.update(|_, cx| set_vim_remaps(cx, &[remap("H", "^")], "\\"));
    assert_eq!(issues, []);
    assert!(maps_h(&first, cx));

    // An editor opened afterwards gets the set too.
    let (second, cx) = open(cx);
    cx.run_until_parked();
    assert!(maps_h(&second, cx));

    // A later call replaces it.
    cx.update(|_, cx| set_vim_remaps(cx, &[remap("L", "$")], "\\"));
    assert!(!maps_h(&first, cx));
    assert!(!maps_h(&second, cx));
}

#[gpui::test]
fn bad_entries_are_reported(cx: &mut TestAppContext) {
    cx.update(init);
    let (view, cx) = open(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    let issues =
        cx.update(|_, cx| set_vim_remaps(cx, &[remap("a", "<Bogus>"), remap("H", "^")], "\\"));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].index, 0, "{issues:?}");
    assert!(maps_h(&view, cx));
}
