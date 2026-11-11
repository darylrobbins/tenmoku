//! Sticky scroll's band in a headless window: scrolled into a function in
//! an impl, the impl's and the function's header rows are pinned at the top
//! with their line numbers, in the colours the text area gives them; the
//! function's row is pushed up as its last row scrolls under it; the scopes
//! are found once per top row; the setting turns the band off live; with
//! the minimap on, the band stops at its left edge; and a long header's
//! slice is widened where it's drawn narrower than indexed, as the text
//! area's is.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_config::{EditorSettings, Settings, USER_SETTINGS_TEMPLATE, settings_schema};
use fe_display::minimap::{MinimapOptions, MinimapShowSlider};
use fe_editor::EditorModel;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, px, size,
};

use fe_display::{RowIndex, RowSlice, layout::MAX_SHAPED_ROW_BYTES};

use super::{Painted, queries, runs, widened_slice};
use crate::{EditorTheme, EditorView, init};

/// The impl's header row, 0-based.
const IMPL: u32 = 4;
/// The function's header row.
const FN: u32 = 5;
/// The function's last row, its closing brace.
const FN_LAST: u32 = 47;

/// Rust with a 43-row function in an impl (rows 4 to 50), then 40 rows of
/// constants.
fn source() -> String {
    let mut text = String::from(
        "struct Point {\n    x: i32,\n}\n\nimpl Point {\n    fn new(x: i32) -> Self {\n",
    );
    for n in 0..40 {
        text.push_str(&format!("        let value_{n} = \"{n}\";\n"));
    }
    text.push_str("        Self { x }\n    }\n\n    fn other(&self) {}\n}\n");
    for n in 0..40 {
        text.push_str(&format!("const C_{n}: u32 = {n};\n"));
    }
    text
}

/// An editor on `text`, as the file at `path`, in an 800 × 400 window, its
/// caret at the start, drawn once parsed.
pub(super) fn editor<'a>(
    text: &str,
    path: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (text, path) = (text.to_owned(), path.to_owned());
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some(path.into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    draw(cx);
    (view, cx)
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(super) fn line_height(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.line_height
    })
}

/// Scrolls `y` pixels down from the top, and draws.
pub(super) fn scroll_to(view: &Entity<EditorView>, y: f32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.scroll.y = px(y);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

/// Scrolls so buffer row `row` (no blocks or folds above it) is at the top.
pub(super) fn scroll_to_row(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    let line_height = line_height(view, cx);
    scroll_to(view, row as f32 * line_height, cx);
}

pub(super) fn band(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    view.read_with(cx, |view, _| view.painted_sticky_scroll.clone())
}

/// The buffer rows pinned, top first, and each row's top from the band's.
pub(super) fn pinned(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, Pixels)> {
    let Some(band) = band(view, cx) else {
        return Vec::new();
    };
    (band.rows.iter())
        .map(|row| (row.buffer_row, row.bounds.origin.y - band.bounds.origin.y))
        .collect()
}

/// What the text area draws for buffer row `row` at the top of the file:
/// its text's origin and colour runs, and its line number's right edge.
pub(super) fn unscrolled(
    view: &Entity<EditorView>,
    row: u32,
    cx: &mut VisualTestContext,
) -> (Point<Pixels>, Vec<(u32, gpui::Hsla)>, Pixels) {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        let shown = (frame.rows.iter())
            .find(|shown| shown.buffer_row == row)
            .expect("the row is shown");
        let origin = layout.to_window(frame.line_origin(shown));
        let (number, line) = &frame.line_numbers[row as usize];
        let right = layout.to_window(*number).x + line.layout.width;
        (origin, runs(&shown.line), right)
    })
}

#[gpui::test]
fn scrolled_into_a_method_its_impl_and_its_header_are_pinned(cx: &mut TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    let line_height = px(line_height(&view, cx));
    let at_top = [IMPL, FN].map(|row| unscrolled(&view, row, cx));

    scroll_to_row(&view, 10, cx);
    let band = band(&view, cx).expect("pinned");
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.)), (FN, line_height)]);
    // Across the gutter and the text, from the editor's top.
    assert_eq!(band.bounds.origin, Point::default());
    assert_eq!(band.bounds.size, size(px(800.), line_height * 2.));
    assert_eq!(band.background, theme.sticky_scroll_background);
    assert_eq!(band.rows[0].text, "impl Point {");
    assert_eq!(band.rows[1].text, "    fn new(x: i32) -> Self {");
    for (row, (origin, runs, number_right)) in band.rows.iter().zip(at_top) {
        // Coloured, placed and numbered as the text area has the row.
        assert!(runs.len() > 2, "{runs:?}: highlighted");
        assert_eq!(row.runs, runs, "{}", row.text);
        assert_eq!(row.text_origin.x, origin.x, "{}", row.text);
        let (number, color) = row.number.expect("a gutter");
        assert_eq!(number.right(), number_right, "{}", row.text);
        assert_eq!(color, theme.gutter_foreground);
    }
    // The impl's row is numbered 5 and the function's 6: one digit each.
    let numbers = band.rows.iter().map(|row| row.number.unwrap().0.size.width);
    assert!(numbers.into_iter().all(|width| width > px(0.)));
    // No border in Tenmoku Dark, and its shadow under the band.
    assert_eq!(band.border, None);
    let shadow = band.shadow.expect("a shadow");
    assert_eq!(shadow.origin.y, band.bounds.bottom());
    assert_eq!(shadow.size, size(px(800.), px(3.)));
}

#[gpui::test]
fn line_numbers_are_the_buffer_rows_one_based_and_active_on_the_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    // The caret on the function's header row.
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(FN);
        view.model.set_selections([ResolvedSelection {
            range: start..start,
            reversed: false,
        }]);
        cx.notify();
    });
    draw(cx);
    scroll_to_row(&view, 10, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let band = band(&view, cx).expect("pinned");
    let colors: Vec<_> = (band.rows.iter())
        .map(|row| row.number.unwrap().1)
        .collect();
    assert_eq!(
        colors,
        [theme.gutter_foreground, theme.gutter_active_foreground]
    );
    // The band's numbers are its rows' own, not the rows under it.
    let rows: Vec<u32> = band.rows.iter().map(|row| row.buffer_row + 1).collect();
    assert_eq!(rows, [5, 6]);
}

#[gpui::test]
fn pinned_numbers_follow_the_line_number_mode(cx: &mut TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    // The caret on row 20, 15 rows under the function's header.
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(20);
        view.model.set_selections([ResolvedSelection {
            range: start..start,
            reversed: false,
        }]);
        cx.notify();
    });
    scroll_to_row(&view, 10, cx);
    let widths = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        let band = band(view, cx).expect("pinned");
        let widths: Vec<_> = (band.rows.iter())
            .map(|row| row.number.map(|(bounds, _)| bounds.size.width))
            .collect();
        widths
    };
    let set = |number, relative, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            let numbers = fe_display::LineNumbers { number, relative };
            view.model.set_line_numbers(numbers);
            cx.notify();
        });
        draw(cx);
    };
    // Rows 5 and 6.
    let [Some(one), Some(_)] = widths(&view, cx)[..] else {
        panic!("absolute numbers");
    };
    // 16 and 15 rows from the caret's, in both relative modes.
    for number in [false, true] {
        set(number, true, cx);
        let [Some(first), Some(second)] = widths(&view, cx)[..] else {
            panic!("relative numbers");
        };
        assert_eq!((first, second), (one * 2., one * 2.));
    }
    set(true, false, cx);
    assert_eq!(widths(&view, cx), [Some(one), Some(one)]);
    // :set nonu nornu: none.
    set(false, false, cx);
    assert_eq!(widths(&view, cx), [None, None]);
}

#[gpui::test]
fn the_function_row_is_pushed_up_as_its_last_row_scrolls_under_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    let line_height = line_height(&view, cx);
    // The function's last row just below the band: both rows in their slots.
    let start = (FN_LAST - 1) as f32 * line_height;
    scroll_to(&view, start, cx);
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.)), (FN, px(line_height))]);
    for by in [1., 5., 13., line_height - 1.] {
        scroll_to(&view, start + by, cx);
        assert_eq!(
            pinned(&view, cx),
            [(IMPL, px(0.)), (FN, px(line_height - by))],
            "scrolled {by} px further"
        );
        let band = band(&view, cx).unwrap();
        assert_eq!(band.bounds.size.height, px(2. * line_height - by));
    }
    // Its last row gone, only the impl is pinned.
    scroll_to(&view, start + line_height, cx);
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.))]);
}

#[gpui::test]
fn the_band_is_empty_at_the_top_and_without_scopes(cx: &mut TestAppContext) {
    {
        let (view, cx) = editor(&source(), "main.rs", cx);
        assert_eq!(band(&view, cx), None, "scroll 0");
        // Among the constants after the impl.
        scroll_to_row(&view, 60, cx);
        assert_eq!(band(&view, cx), None);
    }
    // Rust with no scopes, which doesn't fall back to another model, and
    // text with no syntax and no indented blocks.
    let rows: String = (0..100)
        .map(|n| format!("const C_{n}: u32 = {n};\n"))
        .collect();
    for path in ["main.rs", "notes.txt"] {
        let (view, cx) = editor(&rows, path, cx);
        scroll_to_row(&view, 20, cx);
        assert_eq!(band(&view, cx), None, "{path}");
    }
}

#[gpui::test]
fn scopes_are_found_once_per_top_row_and_version(cx: &mut TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    let line_height = line_height(&view, cx);
    let before = queries();
    scroll_to_row(&view, 10, cx);
    assert_eq!(queries() - before, 1);
    // Within the same top row, a pixel at a time and in jumps.
    for by in [1., 2., 7., 13., line_height - 0.5] {
        scroll_to(&view, 10. * line_height + by, cx);
        assert_eq!(pinned(&view, cx).len(), 2, "{by}");
    }
    assert_eq!(queries() - before, 1);
    // Another top row asks again, and going back does too.
    scroll_to_row(&view, 11, cx);
    assert_eq!(queries() - before, 2);
    scroll_to_row(&view, 10, cx);
    assert_eq!(queries() - before, 3);
    // As does an edit, at the same top row.
    view.update(cx, |view, cx| {
        view.model.insert("// start\n");
        cx.notify();
    });
    draw(cx);
    assert_eq!(queries() - before, 4);
    assert_eq!(pinned(&view, cx).len(), 2);
}

#[gpui::test]
fn the_setting_turns_the_band_off_and_on_live(cx: &mut TestAppContext) {
    let defaults = Settings::default().editor.sticky_scroll;
    assert!(defaults.enabled);
    assert!(
        USER_SETTINGS_TEMPLATE.contains("[editor.sticky_scroll]\n"),
        "the template has the table"
    );
    let (_, table) = USER_SETTINGS_TEMPLATE
        .split_once("[editor.sticky_scroll]\n")
        .unwrap();
    let table = table.split("\n[").next().unwrap();
    assert!(table.contains("\n# enabled = true\n"), "{table}");
    let schema = settings_schema();
    let enabled = &schema["$defs"]["StickyScrollSettings"]["properties"]["enabled"];
    assert_eq!(enabled["type"], "boolean", "{schema:#}");
    assert_eq!(enabled["default"], true, "{schema:#}");

    let (view, cx) = editor(&source(), "main.rs", cx);
    scroll_to_row(&view, 10, cx);
    assert_eq!(pinned(&view, cx).len(), 2);
    let mut settings = EditorSettings::default();
    settings.sticky_scroll.enabled = false;
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    draw(cx);
    assert_eq!(band(&view, cx), None);
    // Recolouring keeps it off.
    view.update(cx, |view, cx| {
        view.set_colors(fe_config::EditorPalette::fastedit_light(), cx)
    });
    draw(cx);
    assert_eq!(band(&view, cx), None);

    settings.sticky_scroll.enabled = true;
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    draw(cx);
    assert_eq!(pinned(&view, cx).len(), 2);
}

#[gpui::test]
fn beside_the_minimap_the_band_ends_at_its_edge(cx: &mut TestAppContext) {
    let (view, cx) = editor(&source(), "main.rs", cx);
    view.update(cx, |view, cx| {
        let options = MinimapOptions {
            show_slider: MinimapShowSlider::Always,
            ..MinimapOptions::default()
        };
        view.set_minimap(Some(options), cx)
    });
    draw(cx);
    scroll_to_row(&view, 10, cx);
    let band = band(&view, cx).expect("pinned");
    let minimap = view.read_with(cx, |view, _| view.minimap_bounds().expect("shown"));
    assert_eq!(band.bounds.right(), minimap.left());
    assert_eq!(band.shadow.unwrap().right(), minimap.left());

    // The slider, and a click on the minimap level with the band (above the
    // slider, which starts at row 10's), are as with the band off.
    let over_band = Point {
        x: minimap.center().x,
        y: band.bounds.top() + px(5.),
    };
    let click = |cx: &mut VisualTestContext| {
        scroll_to_row(&view, 10, cx);
        let slider = view.read_with(cx, |view, _| view.painted_minimap.clone().unwrap().slider);
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: over_band,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: over_band,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        draw(cx);
        let scroll = view.read_with(cx, |view, _| view.scroll.y);
        let hovered = view.read_with(cx, |view, _| view.minimap_hover.over);
        (slider, scroll, hovered)
    };
    let with_band = click(cx);
    assert_ne!(with_band.1, px(10. * line_height(&view, cx)), "scrolled");
    assert!(with_band.2, "the minimap is under the pointer");
    let mut settings = EditorSettings::default();
    settings.sticky_scroll.enabled = false;
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    draw(cx);
    assert_eq!(click(cx), with_band);
}

#[gpui::test]
fn a_long_header_is_pinned_as_the_text_area_draws_it(cx: &mut TestAppContext) {
    // The impl's header past the shaped limit.
    let source = source().replacen(
        "impl Point {",
        &format!("impl Point {{ // {}", "\u{644}\u{627}".repeat(4000)),
        1,
    );
    let (view, cx) = editor(&source, "main.rs", cx);
    let (origin, runs, _) = unscrolled(&view, IMPL, cx);
    let shaped = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        let shown = (frame.rows.iter())
            .find(|shown| shown.buffer_row == IMPL)
            .unwrap();
        shown.expanded.text().to_owned()
    });

    scroll_to_row(&view, 10, cx);
    let band = band(&view, cx).expect("pinned");
    let row = &band.rows[0];
    assert_eq!(row.buffer_row, IMPL);
    // The same slice of the row, coloured and placed as the text area has it.
    assert_eq!(row.text, shaped);
    assert_eq!(row.runs, runs);
    assert_eq!(row.text_origin.x, origin.x);
}

#[test]
fn a_long_row_drawn_narrower_than_indexed_is_widened_to_reach_the_view() {
    let row = "\u{644}\u{627}".repeat(20_000);
    let text = fe_buffer::Text::new(&row);
    let measure = &mut fe_display::Monospace;
    let mut index = RowIndex::new(&text, 0..row.len(), 4, measure);
    let (view, margin) = (1000.0..1100., 32.);
    let chars = |slice: &RowSlice| row[slice.range.clone()].chars().count() as f64;

    // Drawn as indexed, one char a unit: the index's slice, shaped once.
    let mut shapes = 0;
    let (slice, drawn) = widened_slice(&mut index, measure, view.clone(), margin, |slice| {
        shapes += 1;
        (chars(slice), chars(slice))
    });
    assert_eq!(slice, index.slice(view.clone(), margin, measure));
    assert_eq!(shapes, 1);
    assert!(slice.start_x + drawn >= view.end + margin);

    // Drawn a third as wide, as contextual forms can be: widened until the
    // text drawn reaches the view's right edge, in three shapes at most.
    for narrowing in [3., 10.] {
        let mut shapes = 0;
        let (slice, drawn) = widened_slice(&mut index, measure, view.clone(), margin, |slice| {
            shapes += 1;
            (chars(slice) / narrowing, chars(slice) / narrowing)
        });
        assert!(slice.start_x <= view.start, "{narrowing}");
        assert!(slice.start_x + drawn >= view.end, "{narrowing}: {slice:?}");
        assert!(shapes <= 3, "{narrowing}");
        assert!(slice.range.len() <= MAX_SHAPED_ROW_BYTES, "{narrowing}");
    }
}
