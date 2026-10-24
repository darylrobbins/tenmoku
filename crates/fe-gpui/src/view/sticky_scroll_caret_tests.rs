//! Sticky scroll's band never hides the caret: moving the caret up into the
//! rows it covers scrolls so the caret's row is a row below it, and after a
//! vim scroll, which keeps nvim's top row, the band pins nothing at or below
//! the caret's row.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px, size};

use crate::{EditorTheme, EditorView, element::sticky_scroll::Painted, init, vim::set_vim_enabled};

/// The header rows: a module in a module, an impl and a function.
const HEADERS: [u32; 4] = [0, 2, 4, 6];
/// The nested function's header row, inside the function.
const NESTED: u32 = 87;

/// Rust nested four deep, each header a row below a comment: `mod outer`,
/// `mod inner`, `impl S` and `fn f`, whose 80 rows end in a 40-row
/// function nested in it.
fn source() -> String {
    let mut text = String::from(
        "mod outer {\n    // a\n    mod inner {\n        // b\n        impl S {\n            // c\n            fn f() {\n",
    );
    for n in 0..80 {
        text.push_str(&format!("                let v_{n} = {n};\n"));
    }
    text.push_str("                fn nested() {\n");
    for n in 0..40 {
        text.push_str(&format!("                    let w_{n} = {n};\n"));
    }
    text.push_str("                }\n            }\n        }\n    }\n}\n");
    text
}

/// An editor on [`source`] as `main.rs`, in an 800 × 400 window, focused,
/// vim `on`, drawn once parsed.
fn editor(on: bool, cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&source()), Some("main.rs".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        set_vim_enabled(cx, on);
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn line_height(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.line_height
    })
}

/// Puts the caret at the start of buffer row `row`, then scrolls so row
/// `top` is at the top, and draws.
fn caret_and_top(view: &Entity<EditorView>, row: u32, top: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(row);
        view.model.set_cursor(start);
        view.changed(cx);
    });
    draw(cx);
    let line_height = line_height(view, cx);
    view.update(cx, |view, cx| {
        view.scroll.y = px(top as f32 * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

fn band(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    view.read_with(cx, |view, _| view.painted_sticky_scroll.clone())
}

fn pinned(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    let rows = band(view, cx).map(|band| band.rows).unwrap_or_default();
    rows.iter().map(|row| row.buffer_row).collect()
}

/// The band's bottom, from the editor's top: 0 with nothing pinned.
fn band_bottom(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    band(view, cx).map_or(px(0.), |band| band.bounds.size.height)
}

/// The caret's buffer row, and its top from the editor's.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, Pixels) {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        let text = view.model.text();
        let row = text
            .offset_to_point(view.model.primary_selection().head())
            .row;
        let shown = (frame.rows.iter())
            .find(|shown| shown.buffer_row == row)
            .expect("the caret's row is shown");
        let top = layout.to_window(frame.line_origin(shown)).y - layout.origin.y;
        (row, top)
    })
}

/// The buffer row at the top of the view.
fn top_row(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| view.top_buffer_row().expect("drawn"))
}

#[gpui::test]
fn moving_up_into_the_band_scrolls_the_caret_out_from_under_it(cx: &mut TestAppContext) {
    for (on, key) in [(false, "up"), (true, "up"), (true, "k")] {
        let (view, cx) = editor(on, cx);
        let line_height = px(line_height(&view, cx));
        // The caret five rows down, deep in `fn f`: under the band's four
        // rows and the row between.
        caret_and_top(&view, 60, 55, cx);
        assert_eq!(pinned(&view, cx), HEADERS, "vim {on}");
        assert_eq!(caret(&view, cx), (60, line_height * 5.));
        for step in 1..=6 {
            cx.simulate_keystrokes(key);
            draw(cx);
            let (row, top) = caret(&view, cx);
            assert_eq!(row, 60 - step, "vim {on}, {key}");
            // A row below the band, not fe-display's three rows down.
            assert_eq!(pinned(&view, cx), HEADERS, "vim {on}, {key}");
            assert_eq!(
                top,
                band_bottom(&view, cx) + line_height,
                "vim {on}, {key} {step}"
            );
        }
    }
}

#[gpui::test]
fn a_caret_below_the_band_stays_where_it_is(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let line_height = px(line_height(&view, cx));
    caret_and_top(&view, 70, 55, cx);
    cx.simulate_keystrokes("up");
    draw(cx);
    assert_eq!(caret(&view, cx), (69, line_height * 14.));
    assert_eq!(top_row(&view, cx), 55);
}

#[gpui::test]
fn after_zt_the_band_pins_only_what_is_above_the_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(true, cx);
    let line_height = px(line_height(&view, cx));
    let row = NESTED + 10;
    caret_and_top(&view, row, row - 8, cx);
    assert_eq!(pinned(&view, cx).len(), 5, "the nested function's too");

    // nvim's top row, the caret's, and nothing pinned over it.
    cx.simulate_keystrokes("z t");
    draw(cx);
    assert_eq!(top_row(&view, cx), row);
    assert_eq!(caret(&view, cx), (row, px(0.)));
    assert_eq!(pinned(&view, cx), Vec::<u32>::new());
    // Again, which leaves the top row where it is.
    cx.simulate_keystrokes("z t");
    draw(cx);
    assert_eq!(top_row(&view, cx), row);
    assert_eq!(pinned(&view, cx), Vec::<u32>::new());
    // Scrolled back a row at a time, the caret stays and the band pins as
    // many rows as fit above it.
    cx.simulate_keystrokes("ctrl-y");
    draw(cx);
    assert_eq!(top_row(&view, cx), row - 1);
    assert_eq!(caret(&view, cx), (row, line_height));
    assert_eq!(pinned(&view, cx), HEADERS[..1]);
    cx.simulate_keystrokes("ctrl-y ctrl-y");
    draw(cx);
    assert_eq!(top_row(&view, cx), row - 3);
    assert_eq!(pinned(&view, cx), HEADERS[..3]);
    assert!(band_bottom(&view, cx) <= caret(&view, cx).1);

    // The caret's next move autoscrolls past the band again.
    cx.simulate_keystrokes("k");
    draw(cx);
    let (_, top) = caret(&view, cx);
    assert!(top >= band_bottom(&view, cx) + line_height, "{top:?}");
}

#[gpui::test]
fn the_wheel_leaves_the_caret_under_the_band(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let line_height = px(line_height(&view, cx));
    // The caret's row two rows down, under the band, as the wheel left it.
    caret_and_top(&view, 60, 58, cx);
    assert_eq!(caret(&view, cx), (60, line_height * 2.));
    assert_eq!(pinned(&view, cx), HEADERS);
    assert_eq!(top_row(&view, cx), 58);
}

#[gpui::test]
fn a_view_too_short_for_the_band_keeps_the_caret_shown_over_it(cx: &mut TestAppContext) {
    for (on, key) in [(false, "up"), (true, "k")] {
        let (view, cx) = editor(on, cx);
        let line_height = line_height(&view, cx);
        // Eight rows, and the band five deep in the nested function.
        cx.simulate_resize(size(px(800.), px(8. * line_height)));
        draw(cx);
        let row = NESTED + 10;
        caret_and_top(&view, row, row - 6, cx);
        assert_eq!(pinned(&view, cx).len(), 5, "vim {on}");
        for step in 1..=3 {
            cx.simulate_keystrokes(key);
            draw(cx);
            let (at, top) = caret(&view, cx);
            assert_eq!(at, row - step, "vim {on}");
            // Shown whole, with the band over none of it.
            assert!(
                top + px(line_height) <= px(8. * line_height),
                "vim {on}: {top:?}"
            );
            assert!(band_bottom(&view, cx) <= top, "vim {on} {step}: {top:?}");
            // And still so on the next frame.
            draw(cx);
            assert!(
                band_bottom(&view, cx) <= caret(&view, cx).1,
                "vim {on} {step}"
            );
        }
    }
}

#[gpui::test]
fn a_scroll_before_the_frame_overrides_clearing_the_band(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let line_height = line_height(&view, cx);
    // The caret moved under the band, then the view scrolled by the host,
    // before a frame: the host's top stands, the caret under the band.
    caret_and_top(&view, 60, 55, cx);
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(57);
        view.model.set_cursor(start);
        view.changed(cx);
        view.set_scroll_top(56., cx);
    });
    draw(cx);
    assert_eq!(top_row(&view, cx), 56);
    assert_eq!(caret(&view, cx), (57, px(line_height)));
    assert_eq!(pinned(&view, cx), HEADERS);
}

#[gpui::test]
fn a_cancelled_search_puts_back_the_band_as_it_was(cx: &mut TestAppContext) {
    let (view, cx) = editor(true, cx);
    let row = NESTED + 10;
    caret_and_top(&view, row, row - 8, cx);
    cx.simulate_keystrokes("z t");
    draw(cx);
    assert_eq!(pinned(&view, cx), Vec::<u32>::new());
    cx.simulate_keystrokes("/ w _ 3 escape");
    draw(cx);
    assert_eq!(top_row(&view, cx), row);
    assert_eq!(pinned(&view, cx), Vec::<u32>::new());

    // The caret under the band, as a scroll left it: so it stays.
    caret_and_top(&view, 60, 58, cx);
    cx.simulate_keystrokes("/ w _ 3 escape");
    draw(cx);
    assert_eq!(top_row(&view, cx), 58);
    assert_eq!(pinned(&view, cx), HEADERS);
}

#[gpui::test]
fn a_caret_move_while_parsing_clears_the_band_once_parsed(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let line_height = px(line_height(&view, cx));
    caret_and_top(&view, 60, 55, cx);
    // The caret up into the band's rows while the text is being parsed
    // (after an edit, say): the band isn't known yet, so the view waits.
    super::sticky_scroll_click_tests::unparse(&view, cx);
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(58);
        view.model.set_cursor(start);
        view.changed(cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.syntax.as_ref().unwrap().is_parsing()));
    assert_eq!(top_row(&view, cx), 55);
    assert!(view.read_with(cx, |view, _| view.clear_band));
    // Parsed, a row below the band.
    draw(cx);
    draw(cx);
    assert_eq!(pinned(&view, cx), HEADERS);
    let (row, top) = caret(&view, cx);
    assert_eq!(row, 58);
    assert_eq!(top, band_bottom(&view, cx) + line_height);
}

/// The top of the caret the last frame painted, from the editor's.
fn painted_caret_top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let carets = &view.painted_carets;
        assert_eq!(carets.len(), 1, "{carets:?}");
        carets[0].top() - layout.origin.y
    })
}

#[gpui::test]
fn the_band_clears_vims_cursor_in_visual_mode(cx: &mut TestAppContext) {
    let (view, cx) = editor(true, cx);
    let line_height = px(line_height(&view, cx));
    // A block selected upward from row 60: its cursor, painted on its top
    // row, not the model's head below, is kept a row below the band.
    caret_and_top(&view, 60, 55, cx);
    cx.simulate_keystrokes("ctrl-v k k");
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("visual_block")
    );
    assert_eq!(pinned(&view, cx), HEADERS);
    assert_eq!(
        painted_caret_top(&view, cx),
        band_bottom(&view, cx) + line_height
    );

    // Charwise to the row's end, then `zt`: the cursor's row is the top,
    // with nothing pinned over it, though the model's head is the next row.
    cx.simulate_keystrokes("escape");
    let row = NESTED + 10;
    caret_and_top(&view, row, row - 8, cx);
    cx.simulate_keystrokes("v $ z t");
    draw(cx);
    assert_eq!(top_row(&view, cx), row);
    assert_eq!(painted_caret_top(&view, cx), px(0.));
    assert_eq!(pinned(&view, cx), Vec::<u32>::new());
}

/// Where the frame draws buffer row `row`'s top, from the editor's.
fn row_top(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Option<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        let shown = frame.rows.iter().find(|shown| shown.buffer_row == row)?;
        Some(layout.to_window(frame.line_origin(shown)).y - layout.origin.y)
    })
}

#[gpui::test]
fn a_search_preview_while_parsing_clears_the_band_off_its_match(cx: &mut TestAppContext) {
    let (view, cx) = editor(true, cx);
    let line_height = px(line_height(&view, cx));
    // Searching back from the nested function's end to `w_5`, five rows
    // into it, while the text is being parsed.
    let (from, found) = (NESTED + 40, NESTED + 6);
    caret_and_top(&view, from, from - 7, cx);
    cx.simulate_keystrokes("?");
    super::sticky_scroll_click_tests::unparse(&view, cx);
    view.update(cx, |view, cx| view.preview_search("w_5", cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(view.read_with(cx, |view, _| view.syntax.as_ref().unwrap().is_parsing()));
    assert!(row_top(&view, found, cx).is_some(), "previewed");
    // Parsed, the match is a row below the band, the caret where it was.
    draw(cx);
    draw(cx);
    assert_eq!(pinned(&view, cx).len(), 5);
    let top = row_top(&view, found, cx).expect("still shown");
    assert_eq!(top, band_bottom(&view, cx) + line_height);
    let caret_row = view.read_with(cx, |view, _| {
        let model = view.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    });
    assert_eq!(caret_row, from);
}
