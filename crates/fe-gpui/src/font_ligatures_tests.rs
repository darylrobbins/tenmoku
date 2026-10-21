//! Ligatures: `editor.font_ligatures` and the editor's font.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::*;

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("a => b -> c"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    })
}

fn calt(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<bool> {
    view.read_with(cx, |view, _| view.theme().font().features.is_calt_enabled())
}

#[gpui::test]
fn default_font_has_calt_off(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    assert_eq!(calt(&view, cx), Some(false));
}

#[gpui::test]
fn ligatures_setting_leaves_calt_alone(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let settings = EditorSettings {
        font_ligatures: true,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    assert_ne!(calt(&view, cx), Some(false));
}

#[gpui::test]
fn flipping_the_setting_changes_an_open_editor(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.run_until_parked();
    let off = view.read_with(cx, |view, _| view.theme().font());
    let on = EditorSettings {
        font_ligatures: true,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&on, cx));
    let shaped = view.read_with(cx, |view, _| view.theme().font());
    assert_ne!(shaped, off);
    view.update(cx, |view, cx| {
        view.apply_settings(&EditorSettings::default(), cx)
    });
    assert_eq!(view.read_with(cx, |view, _| view.theme().font()), off);
    // A recolour keeps the setting.
    view.update(cx, |view, cx| view.apply_settings(&on, cx));
    view.update(cx, |view, cx| {
        view.set_colors(EditorPalette::fastedit_light(), cx)
    });
    assert_ne!(view.read_with(cx, |view, _| view.theme().font()), off);
}
