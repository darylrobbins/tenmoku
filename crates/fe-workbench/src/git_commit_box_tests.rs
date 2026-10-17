//! The commit box's size: 3 rows tall at least, and wrapped at its own
//! width, with no gutter taking room from its text.

use gpui::TestAppContext;

use super::{box_height, draw, fixture, open, set_text};
use crate::git_panel::HINT_ROOM;

#[gpui::test]
fn an_empty_box_is_three_rows_tall(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, _, commit_box, cx) = open(cx, dir.path());
    draw(cx);
    let empty = box_height(&commit_box, cx);
    assert!((3.0..3.1).contains(&empty), "{empty}");

    set_text(&commit_box, "Subject\nBody", cx);
    let two = box_height(&commit_box, cx);
    assert!((3.0..3.1).contains(&two), "{two}");
}

#[gpui::test]
fn the_box_wraps_at_its_own_width_with_no_gutter(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, _, commit_box, cx) = open(cx, dir.path());
    draw(cx);
    let wrap = commit_box.read_with(cx, |editor, _| editor.model().display().wrap_width());
    let wrap = wrap.expect("the box soft-wraps");
    let (font, font_size) = commit_box.read_with(cx, |editor, _| {
        let theme = editor.theme();
        (gpui::font(theme.font_family.clone()), theme.font_size)
    });
    // The em as the editor's element measures it.
    let em: f32 = cx.update(|window, _| {
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        text_system
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6)
            .into()
    });
    let bounds = cx.debug_bounds("commit-box").expect("the box is drawn");
    let width: f32 = bounds.size.width.into();
    // A gutter alone would take 9.75 ems.
    let least = width - (HINT_ROOM + 3.) * em;
    assert!(
        wrap >= least,
        "wraps at {wrap}, in a box {width} wide, em {em}"
    );
}
