//! The editor's colours from a palette, and the palette in use.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_syntax::Highlight;
use gpui::TestAppContext;

use crate::*;

#[gpui::test]
fn from_settings_is_fastedit_dark(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings = EditorSettings::default();
        assert_eq!(
            EditorTheme::from_settings(&settings, cx),
            EditorTheme::from_palette(EditorPalette::fastedit_dark(), &settings, cx)
        );
        assert_eq!(*editor_palette(cx), *EditorPalette::fastedit_dark());
        let light = Arc::new(EditorPalette::fastedit_light().clone());
        set_editor_palette(cx, light.clone());
        assert_eq!(editor_palette(cx), light);
    });
}

#[gpui::test]
fn set_colors_keeps_metrics(cx: &mut TestAppContext) {
    cx.update(init);
    let settings = EditorSettings {
        font_size: 20.,
        line_height: 1.5,
        font_family: Some("Menlo".into()),
        ..EditorSettings::default()
    };
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new("fn main() {}"), None);
        EditorView::new(model, EditorTheme::from_settings(&settings, cx), cx)
    });
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let before = view.read_with(cx, |view, _| view.theme().clone());
    view.update(cx, |view, cx| {
        view.set_colors(EditorPalette::fastedit_light(), cx)
    });
    let after = view.read_with(cx, |view, _| view.theme().clone());
    assert_ne!(after.background, before.background);
    assert_ne!(after.popover_background, before.popover_background);
    assert_ne!(after.syntax_color(keyword), before.syntax_color(keyword));
    assert!(after.syntax_color(keyword).is_some());
    assert_eq!(
        (
            after.font_family.clone(),
            after.font_size,
            after.line_height
        ),
        (
            before.font_family.clone(),
            before.font_size,
            before.line_height
        )
    );
    assert_eq!(after.font_family.as_ref(), "Menlo");
}
