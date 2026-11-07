//! The `.editorconfig` `max_line_length` ruler: painted beside
//! `editor.rulers`, in any wrap mode, and gone when the limit is.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::PAINTED;
use crate::{EditorSettings, EditorTheme, EditorView, init};

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text = "x".repeat(400);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The x of each ruler the last frame painted.
fn painted(cx: &mut VisualTestContext) -> Vec<f32> {
    draw(cx);
    PAINTED.with(|painted| painted.borrow().iter().map(|r| r.origin.x.into()).collect())
}

#[gpui::test]
fn max_line_length_draws_a_ruler(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    // Small enough that 120 columns fit.
    let settings = EditorSettings {
        font_size: 8.,
        rulers: vec![100],
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    let [only] = painted(cx)[..] else {
        panic!("one ruler from editor.rulers")
    };

    view.update(cx, |view, cx| view.set_max_line_length(Some(60), cx));
    let both = painted(cx);
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both[0] < both[1], "60 is left of 100: {both:?}");
    assert_eq!(both[1], only);

    // Settings changes leave it be, and the same column is drawn once.
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    assert_eq!(painted(cx), both);
    view.update(cx, |view, cx| view.set_max_line_length(Some(100), cx));
    assert_eq!(painted(cx), [only]);

    // Gone with the limit.
    view.update(cx, |view, cx| view.set_max_line_length(None, cx));
    assert_eq!(painted(cx), [only]);
}
