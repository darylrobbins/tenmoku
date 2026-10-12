//! The status bar's indentation item follows the style the editor detected
//! in its file, not the tab size setting.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::ItemHandle;

use super::status;
use crate::{EditorSettings, EditorTheme, EditorView};

/// The indentation item's text.
fn indent_item(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    let item: Box<dyn ItemHandle> = Box::new(view.clone());
    let items = cx.update(|_, cx| item.status_items(cx));
    let indent = items.iter().find(|item| item.id == status::INDENT);
    indent.expect("an indentation item").text.to_string()
}

#[gpui::test]
fn shows_the_detected_style(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let mut editor = |text: String| {
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let model = EditorModel::new(Buffer::new(&text), None);
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        });
        (view, cx.clone())
    };
    let (tabs, mut tabs_cx) = editor("a {\n\tb\n}\n".repeat(5));
    let (spaces, mut spaces_cx) = editor("a {\n  b\n}\n".repeat(5));
    assert_eq!(indent_item(&tabs, &mut tabs_cx), "Tab Size: 4");
    assert_eq!(indent_item(&spaces, &mut spaces_cx), "Spaces: 2");

    let settings = EditorSettings {
        tab_size: 8,
        ..EditorSettings::default()
    };
    spaces.update(&mut spaces_cx, |view, cx| {
        view.apply_settings(&settings, cx)
    });
    assert_eq!(indent_item(&spaces, &mut spaces_cx), "Spaces: 2");
    tabs.update(&mut tabs_cx, |view, cx| view.apply_settings(&settings, cx));
    assert_eq!(indent_item(&tabs, &mut tabs_cx), "Tab Size: 8");
}
