//! `InlayHintKind::Closing` draws faint text without a pill.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView, InlayHint, InlayHintKind};
use gpui::{Focusable as _, TestAppContext, px};

const TEXT: &str = "fn main() {\n    let x = 1;\n}\n";

fn hint(kind: InlayHintKind) -> InlayHint {
    InlayHint {
        offset: TEXT.trim_end().len(),
        label: "fn main".into(),
        kind: Some(kind),
        padding_left: true,
        padding_right: false,
        data: None,
        tooltip: None,
        edits: Vec::new(),
    }
}

fn drawn(kind: InlayHintKind, cx: &mut TestAppContext) -> fe_gpui::DrawnPill {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), Some("main.rs".into()));
        let mut theme = EditorTheme::default_dark(cx);
        theme.font_size = px(13.);
        EditorView::new(model, theme, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        view.set_inlay_hints(version, vec![hint(kind)]);
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let annotations = view.drawn_annotations().expect("drawn");
        assert_eq!(annotations.inlay_hints.len(), 1);
        annotations.inlay_hints[0].clone()
    })
}

#[gpui::test]
fn the_closing_kind_is_faint_text_without_a_pill(cx: &mut TestAppContext) {
    let closing = drawn(InlayHintKind::Closing, cx);
    assert_eq!(closing.text.text.as_ref(), "fn main");
    assert_eq!(closing.background.a, 0.);
    assert_eq!(closing.corner_radius, px(0.));
    let theme_faint = closing.text.color;
    let type_hint = drawn(InlayHintKind::Type, cx);
    assert!(type_hint.background.a > 0.);
    assert_eq!(type_hint.text.color, theme_faint);
}
