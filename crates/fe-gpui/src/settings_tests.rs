//! Applying settings to open editors, in a headless window.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_config::Settings;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::*;

/// A focused editor on a long row, with the default settings.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text = "word ".repeat(400);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

fn next_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
}

/// A reload from `previous` to `settings`.
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

#[gpui::test]
fn apply_settings_updates_open_views(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let settings = EditorSettings {
        font_size: 20.,
        line_height: 1.5,
        tab_size: 2,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    next_frame(cx);
    let (row_height, tab_size) = view.update(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        (frame.line_height, view.model().tab_size())
    });
    assert_eq!(row_height, 30.);
    assert_eq!(tab_size, 2);

    let soft_wrap =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model().soft_wrap());
    let rows =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model().display().row_count());

    // A reload of the font alone keeps the editor's alt-z toggle, and
    // re-measures the wrap.
    cx.simulate_keystrokes("alt-z");
    next_frame(cx);
    let narrow = rows(cx);
    let bigger = EditorSettings {
        font_size: 40.,
        ..settings.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&settings, &bigger), cx)
    });
    next_frame(cx);
    assert!(soft_wrap(cx), "the alt-z toggle survives");
    assert!(
        rows(cx) > narrow,
        "{} rows at 40 px, {narrow} at 20 px",
        rows(cx)
    );

    // A reload that changes soft_wrap applies it.
    cx.simulate_keystrokes("alt-z");
    assert!(!soft_wrap(cx));
    let wrapped = EditorSettings {
        soft_wrap: true,
        ..bigger.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&bigger, &wrapped), cx)
    });
    assert!(soft_wrap(cx));

    let (default_dark, from_defaults) = cx.update(|_, cx| {
        (
            EditorTheme::default_dark(cx),
            EditorTheme::from_settings(&EditorSettings::default(), cx),
        )
    });
    assert_eq!(default_dark, from_defaults);
}
