//! `editor.cursor_surrounding_lines` in the sticky-scroll clearance: the
//! margin element.rs keeps below the caret is the option's, and a settings
//! reload applies it live.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_config::Settings;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use crate::{EditorTheme, EditorView, init, vim::set_vim_enabled, *};

/// Rust nested four deep, so the sticky band pins four rows over `fn f`'s
/// 80.
fn source() -> String {
    let mut text = String::from(
        "mod outer {\n    // a\n    mod inner {\n        // b\n        impl S {\n            // c\n            fn f() {\n",
    );
    for n in 0..80 {
        text.push_str(&format!("                let v_{n} = {n};\n"));
    }
    text.push_str("            }\n        }\n    }\n}\n");
    text
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&source()), Some("main.rs".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        set_vim_enabled(cx, false);
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
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

/// The rows from the top of the view to the caret's row after the caret
/// moves up from row 60 with the view's top at row 55, in rows.
fn caret_row_in_view(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(60);
        view.model.set_cursor(start);
        view.changed(cx);
    });
    draw(cx);
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.line_height
    });
    view.update(cx, |view, cx| {
        view.scroll.y = px(55. * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("up");
    draw(cx);
    view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        let top = frame.scroll.y / frame.line_height;
        let row = view
            .model
            .text()
            .offset_to_point(view.model.primary_selection().head())
            .row;
        row as f32 - top
    })
}

#[gpui::test]
fn sticky_margin_follows_the_option(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let defaults = EditorSettings::default();
    // Four pinned rows and the one between put the caret five rows down
    // whatever the margin is below that; 8 rows is the option's own, and a
    // huge one centres the caret in the 400px window's rows.
    for (so, want) in [(0, 5.), (3, 5.), (8, 8.), (100, 9.02)] {
        let settings = EditorSettings {
            cursor_surrounding_lines: so,
            ..defaults.clone()
        };
        view.update(cx, |view, cx| {
            view.reload_settings(&event(&defaults, &settings), cx)
        });
        assert_eq!(view.read_with(cx, |view, _| view.model().scroll_off()), so);
        let row = caret_row_in_view(&view, cx);
        assert!((row - want).abs() < 0.01, "so {so}: {row}");
        // Back to the default by a reload too.
        view.update(cx, |view, cx| {
            view.reload_settings(&event(&settings, &defaults), cx)
        });
        assert_eq!(view.read_with(cx, |view, _| view.model().scroll_off()), 3);
    }
}
