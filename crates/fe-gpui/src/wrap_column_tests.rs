//! `editor.wrap_column` and `editor.wrap_mode` in a headless window.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_config::{
    Settings,
    settings::{EditorSettings, WrapMode},
};
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::*;

/// A focused editor on one 100-char row, soft wrap on.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(&"x".repeat(100)), None);
        model.set_soft_wrap(true);
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

fn em(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, cx| {
        let view = view.read(cx);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&gpui::font(view.theme.font_family.clone()));
        text_system
            .em_advance(font_id, view.theme.font_size)
            .unwrap()
            .into()
    })
}

/// Resizes the window to `columns` cells plus room for the gutter.
fn resize(columns: f32, view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let em = em(view, cx);
    let height = cx.update(|window, _| window.viewport_size().height);
    cx.simulate_resize(gpui::size(gpui::px((columns + 12.) * em), height));
    cx.run_until_parked();
    next_frame(cx);
}

fn configure(
    view: &Entity<EditorView>,
    mode: WrapMode,
    column: u32,
    soft_wrap: bool,
    cx: &mut VisualTestContext,
) {
    let settings = EditorSettings {
        wrap_column: column,
        wrap_mode: mode,
        soft_wrap,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    next_frame(cx);
}

fn rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.update(cx, |view, _| view.model().display().row_count())
}

fn wrap_width(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<f32> {
    view.update(cx, |view, _| view.model().display().wrap_width())
}

#[gpui::test]
fn column_mode_wraps_at_n_columns_in_a_wider_window(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    resize(120., &view, cx);
    configure(&view, WrapMode::Viewport, 40, true, cx);
    assert_eq!(rows(&view, cx), 1, "the viewport fits the row");
    configure(&view, WrapMode::Column, 40, true, cx);
    assert_eq!(rows(&view, cx), 3);
    let em = em(&view, cx);
    assert_eq!(wrap_width(&view, cx), Some(40. * em));
    view.update(cx, |view, _| {
        let row = view.model().display().row(0);
        assert_eq!(row.range.len(), 40);
    });
}

#[gpui::test]
fn bounded_mode_takes_the_smaller_width(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let em = em(&view, cx);

    resize(30., &view, cx);
    configure(&view, WrapMode::Viewport, 40, true, cx);
    let viewport = wrap_width(&view, cx).unwrap();
    assert!(viewport < 40. * em, "{viewport}");
    configure(&view, WrapMode::Bounded, 40, true, cx);
    assert_eq!(wrap_width(&view, cx), Some(viewport));
    assert!(rows(&view, cx) > 3);

    resize(120., &view, cx);
    assert_eq!(wrap_width(&view, cx), Some(40. * em));
    assert_eq!(rows(&view, cx), 3);
}

#[gpui::test]
fn soft_wrap_off_ignores_the_column(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    resize(120., &view, cx);
    for mode in [WrapMode::Viewport, WrapMode::Column, WrapMode::Bounded] {
        configure(&view, mode, 40, false, cx);
        view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
        next_frame(cx);
        assert_eq!(wrap_width(&view, cx), None, "{mode:?}");
        assert_eq!(rows(&view, cx), 1, "{mode:?}");
    }
    // alt-z still toggles, and the column applies once it's on.
    configure(&view, WrapMode::Column, 40, false, cx);
    cx.simulate_keystrokes("alt-z");
    next_frame(cx);
    assert!(view.update(cx, |view, _| view.model().soft_wrap()));
    assert_eq!(rows(&view, cx), 3);
    cx.simulate_keystrokes("alt-z");
    next_frame(cx);
    assert!(!view.update(cx, |view, _| view.model().soft_wrap()));
    assert_eq!(rows(&view, cx), 1);
}

#[gpui::test]
fn changing_the_setting_rewraps_on_reload(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    resize(120., &view, cx);
    let before = EditorSettings {
        soft_wrap: true,
        wrap_column: 40,
        wrap_mode: WrapMode::Column,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&before, cx));
    next_frame(cx);
    assert_eq!(rows(&view, cx), 3);

    let after = EditorSettings {
        wrap_column: 30,
        ..before.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&before, &after), cx)
    });
    next_frame(cx);
    assert_eq!(rows(&view, cx), 4);

    let off = EditorSettings {
        wrap_mode: WrapMode::Viewport,
        ..after.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&after, &off), cx)
    });
    next_frame(cx);
    assert_eq!(rows(&view, cx), 1);
}

#[gpui::test]
fn a_column_wider_than_the_window_scrolls_to_the_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    resize(30., &view, cx);
    configure(&view, WrapMode::Column, 80, true, cx);
    assert_eq!(rows(&view, cx), 2);
    let scroll = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let frame = &view.layout.as_ref().expect("drawn").frame;
            (
                frame.scroll.x,
                frame.cursors.first().map(|caret| caret.x),
                frame.width,
            )
        })
    };
    assert_eq!(scroll(cx).0, 0.);

    // The caret at the end of the first display row is 80 columns out, past
    // the window's edge: the view follows it.
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(79);
        view.changed(cx);
    });
    next_frame(cx);
    let (x, caret, width) = scroll(cx);
    assert!(x > 0., "scrolled {x}");
    let caret = caret.expect("a caret");
    assert!(caret >= 0. && caret <= width, "{caret} in {width}");

    // Back at the start, it scrolls back.
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(0);
        view.changed(cx);
    });
    next_frame(cx);
    assert_eq!(scroll(cx).0, 0.);

    // A window wide enough for the column doesn't scroll.
    resize(120., &view, cx);
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(79);
        view.changed(cx);
    });
    next_frame(cx);
    assert_eq!(scroll(cx).0, 0.);
}

#[gpui::test]
fn a_split_keeps_the_wrap_settings(cx: &mut TestAppContext) {
    use kiln_shell::ItemHandle;

    let (view, cx) = editor(cx);
    resize(120., &view, cx);
    configure(&view, WrapMode::Column, 40, true, cx);
    let item: Box<dyn ItemHandle> = Box::new(view.clone());
    let split = cx
        .update(|window, cx| item.split(window, cx))
        .expect("splits");
    let split = split.downcast::<EditorView>().expect("an editor");
    cx.run_until_parked();
    split.read_with(cx, |split, _| {
        assert_eq!(split.wrap_column, Some(40));
        assert_eq!(split.wrap_mode, WrapMode::Column);
    });

    // An editor made from settings wraps by them without an apply.
    let settings = EditorSettings {
        wrap_column: 40,
        wrap_mode: WrapMode::Column,
        ..EditorSettings::default()
    };
    view.update(cx, |view, _| {
        view.wrap_column = None;
        view.wrap_mode = WrapMode::Viewport;
        view.set_wrap_settings(&settings);
        assert_eq!(view.wrap_column, Some(40));
        assert_eq!(view.wrap_mode, WrapMode::Column);
    });
}
