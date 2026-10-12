//! A counted `ctrl-d`/`ctrl-u` doesn't outlive a resize away and back,
//! even with no scroll key between.

use gpui::{TestAppContext, px, size};

use super::scroll_tests::{caret_row, draw, editor, go_to_row};

#[gpui::test]
fn resize_away_and_back_drops_counted_scroll(cx: &mut TestAppContext) {
    for plain in ["ctrl-d", "ctrl-u"] {
        let (view, cx) = editor(cx, 100, true);
        let line_height = draw(&view, cx).1;
        go_to_row(&view, 50, cx);
        cx.simulate_keystrokes("3 ctrl-d");
        assert_eq!(caret_row(&view, cx), 53);
        cx.simulate_resize(size(px(600.), px(line_height * 4.)));
        assert_eq!(draw(&view, cx).2, 4);
        cx.simulate_resize(size(px(600.), px(line_height * 10.)));
        assert_eq!(draw(&view, cx).2, 10);
        let row = caret_row(&view, cx);
        cx.simulate_keystrokes(plain);
        assert_eq!(caret_row(&view, cx).abs_diff(row), 5, "{plain}");
    }
}
