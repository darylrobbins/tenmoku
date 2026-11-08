//! The design's families and UI size (T1515,
//! `docs/design/design-system.md#type-spacing-and-sizes`): the workbench's
//! text is Instrument Sans at 13/18, and the editor's popovers set their
//! prose in that family.

use fe_ui::FontFamilies;
use gpui::{TestAppContext, px};

use super::open_root;

#[gpui::test]
fn workbench_text_is_instrument_sans_13(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // What `fe_workbench::init` sets for kiln's root and the editor.
    let families = cx.update(|_, cx| cx.global::<FontFamilies>().clone());
    assert_eq!(families.ui, fe_fonts::UI);
    assert_eq!(families.code.as_deref(), Some(fe_fonts::CODE));
    assert_eq!(families.title, fe_fonts::TITLE);

    // The text the workspace's content inherits.
    let style = workbench.read_with(cx, |workbench, _| {
        workbench.content_text_style.borrow().clone()
    });
    let style = style.expect("the content was drawn");
    assert_eq!(style.font_family, "Instrument Sans");
    let rem = cx.update(|window, _| window.rem_size());
    assert_eq!(style.font_size.to_pixels(rem), px(13.));
    assert_eq!(style.line_height.to_pixels(style.font_size, rem), px(18.));

    // The overlays over the workspace, the setup sheet and the toasts,
    // inherit the family too: they aren't under the content's div.
    let overlay = workbench.read_with(cx, |workbench, _| {
        workbench.overlay_text_style.borrow().clone()
    });
    let overlay = overlay.expect("the toasts' layer was drawn");
    assert_eq!(overlay.font_family, "Instrument Sans");

    // Hover, completion and signature help set their prose in it.
    assert_eq!(
        cx.update(|_, cx| fe_gpui::ui_font_family(cx)),
        "Instrument Sans"
    );
}
