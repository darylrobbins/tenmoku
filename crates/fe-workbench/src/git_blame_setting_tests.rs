//! `editor.inline_blame`: line blame starting on, and following a reload.

use std::sync::Arc;

use fe_config::{Settings, SettingsEvent};
use fe_gpui::EditorSettings;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{
    annotations, dispatch, expected, fixture, put_caret, settle, tab_editor, window, window_with,
};
use crate::Workbench;

fn on() -> EditorSettings {
    EditorSettings {
        inline_blame: true,
        ..EditorSettings::default()
    }
}

/// Reloads settings whose `editor.inline_blame` goes `from` to `to`.
fn reload(workbench: &Entity<Workbench>, from: bool, to: bool, cx: &mut VisualTestContext) {
    let mut before = Settings::default();
    before.editor.inline_blame = from;
    let mut after = Settings::default();
    after.editor.inline_blame = to;
    let event = SettingsEvent {
        settings: Arc::new(after),
        previous: Arc::new(before),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    settle(cx);
}

#[gpui::test]
fn the_setting_starts_line_blame(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();

    let (workbench, mut vcx) = window_with(cx, root, Some("a.rs"), on());
    let tab = tab_editor(&workbench, &mut vcx);
    put_caret(&tab, 1, &mut vcx);
    settle(&mut vcx);
    assert_eq!(annotations(&tab, &mut vcx), [(1, expected(root, 1))]);
    drop(vcx);

    let (workbench, mut vcx) = window(cx, root, Some("a.rs"));
    let cx = &mut vcx;
    let tab = tab_editor(&workbench, cx);
    put_caret(&tab, 1, cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [], "off by default");
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(1, expected(root, 1))]);
}

#[gpui::test]
fn changing_the_setting_follows(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root, Some("a.rs"));
    let cx = &mut vcx;
    let tab = tab_editor(&workbench, cx);
    put_caret(&tab, 1, cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), []);

    reload(&workbench, false, true, cx);
    assert_eq!(annotations(&tab, cx), [(1, expected(root, 1))]);

    reload(&workbench, true, false, cx);
    assert_eq!(annotations(&tab, cx), []);

    // A reload that leaves it alone keeps the window's own toggle.
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    assert_eq!(annotations(&tab, cx), [(1, expected(root, 1))]);
    reload(&workbench, false, false, cx);
    assert_eq!(annotations(&tab, cx), [(1, expected(root, 1))]);
    dispatch("git.toggle_line_blame", cx);
    reload(&workbench, false, false, cx);
    assert_eq!(annotations(&tab, cx), []);
}
