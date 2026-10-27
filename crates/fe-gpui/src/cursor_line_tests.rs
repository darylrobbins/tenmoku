//! `editor.cursor_line` and `:set cursorline` in a headless window: the
//! band follows the option, a settings reload applies it, and an editor's
//! own `:set` survives a reload of something else.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_config::Settings;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{commands::RunCommand, *};

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new("one\ntwo\nthree"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn next_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
}

fn event(previous: &EditorSettings, settings: &EditorSettings) -> SettingsEvent {
    let wrap = |editor: &EditorSettings| {
        Arc::new(Settings {
            editor: editor.clone(),
            ..Settings::default()
        })
    };
    SettingsEvent {
        settings: wrap(settings),
        previous: wrap(previous),
        diagnostics: Vec::new(),
    }
}

fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": line }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    next_frame(cx);
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.paints_cursor_line())
}

#[gpui::test]
fn band_follows_the_option(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    next_frame(cx);
    assert!(painted(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(
            !view
                .layout
                .as_ref()
                .expect("drawn")
                .frame
                .active_lines
                .is_empty()
        );
    });

    ex("set nocul", cx);
    assert!(!painted(&view, cx));
    ex("set cul", cx);
    assert!(painted(&view, cx));

    // A reload that changes editor.cursor_line applies it.
    let defaults = EditorSettings::default();
    let off = EditorSettings {
        cursor_line: false,
        ..defaults.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&defaults, &off), cx)
    });
    next_frame(cx);
    assert!(!painted(&view, cx));
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&off, &defaults), cx)
    });
    next_frame(cx);
    assert!(painted(&view, cx));
}

#[gpui::test]
fn set_survives_an_unrelated_reload(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    ex("set nocul", cx);
    let defaults = EditorSettings::default();
    let bigger = EditorSettings {
        font_size: defaults.font_size + 1.,
        ..defaults.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&defaults, &bigger), cx)
    });
    next_frame(cx);
    assert!(!painted(&view, cx));
}
