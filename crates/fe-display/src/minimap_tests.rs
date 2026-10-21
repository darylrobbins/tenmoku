use super::*;

/// A 600px editor of 20px rows (30 visible) and 8px columns, 1200px wide.
fn viewport(rows: u32, first_visible_row: f32) -> MinimapViewport {
    MinimapViewport {
        width: 1200.,
        height: 600.,
        em: 8.,
        rows,
        first_visible_row,
        visible_rows: 30.,
    }
}

fn sized(size: MinimapSize) -> MinimapOptions {
    MinimapOptions {
        size,
        ..MinimapOptions::default()
    }
}

fn layout(size: MinimapSize, rows: u32, first: f32) -> MinimapLayout {
    MinimapLayout::new(&sized(size), &viewport(rows, first))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[track_caller]
fn assert_close(a: f32, b: f32) {
    assert!(close(a, b), "{a} != {b}");
}

#[test]
fn width_is_max_column_times_scale() {
    let options = MinimapOptions::default();
    assert_eq!(options.column_width(), 1.);
    assert_eq!(options.row_height(), 2.);
    assert_eq!(options.width(1200., 8.), 120.);
    // floor(1000 / 9) columns leaves the text as many.
    assert_eq!(options.width(1000., 8.), 111.);
    for (scale, max_column, width) in [(2, 120, 240.), (3, 120, 327.), (3, 80, 240.)] {
        let options = MinimapOptions {
            scale,
            max_column,
            ..options
        };
        assert_eq!(options.column_width(), scale as f32);
        assert_eq!(options.row_height(), 2. * scale as f32);
        // At scale 3 the narrow cap, floor(1200 * 3 / 11), is 327.
        assert_eq!(options.width(1200., 8.), width, "{scale} {max_column}");
    }
    // Blocks are a third taller, as VS Code's are.
    let blocks = MinimapOptions {
        render_characters: false,
        scale: 2,
        ..options
    };
    assert_eq!(blocks.row_height(), 6.);
    // Out-of-range scales are clamped.
    let big = MinimapOptions {
        scale: 9,
        ..options
    };
    assert_eq!(big.column_width(), 3.);
}

#[test]
fn a_narrow_editor_shrinks_then_hides_the_minimap() {
    let options = MinimapOptions::default();
    // floor(450 * 1 / (8 + 1)) = 50 columns: as many as the text gets.
    assert_eq!(options.width(450., 8.), 50.);
    let scaled = MinimapOptions {
        scale: 2,
        ..options
    };
    assert_eq!(scaled.width(450., 8.), 90.);
    // Two columns show; fewer hide it.
    assert_eq!(options.width(18., 8.), 2.);
    assert_eq!(options.width(17., 8.), 0.);
    assert_eq!(scaled.width(19., 8.), 0.);
    let one = MinimapOptions {
        max_column: 1,
        ..options
    };
    assert_eq!(one.width(1200., 8.), 0.);

    let mut narrow = viewport(10, 0.);
    narrow.width = 17.;
    let layout = MinimapLayout::new(&options, &narrow);
    assert!(!layout.visible());
    assert_eq!(layout.text_x, 0.0..17.);
}

#[test]
fn side_places_the_minimap_and_the_text_takes_the_rest() {
    let right = layout(MinimapSize::Proportional, 10, 0.);
    assert!(right.visible());
    assert_eq!(right.x, 1080.0..1200.);
    assert_eq!(right.text_x, 0.0..1080.);
    let options = MinimapOptions {
        side: MinimapSide::Left,
        ..MinimapOptions::default()
    };
    let left = MinimapLayout::new(&options, &viewport(10, 0.));
    assert_eq!(left.x, 0.0..120.);
    assert_eq!(left.text_x, 120.0..1200.);
}

#[test]
fn proportional_scrolls_a_long_file_with_the_editor() {
    // 1000 rows and 29 past the end are 2058px; the minimap is 600.
    let top = layout(MinimapSize::Proportional, 1000, 0.);
    assert_eq!(top.row_height, 2.);
    assert_eq!(top.top, 0.);
    assert_eq!((top.first_row, top.first_row_y, top.end_row), (0, 0., 300));
    assert_eq!((top.slider_top, top.slider_height), (0., 60.));

    let middle = layout(MinimapSize::Proportional, 1000, 499.5);
    assert_close(middle.top, (2058. - 600.) / 2.);
    assert_eq!(middle.first_row, 364);
    assert_close(middle.first_row_y, -1.);
    assert_eq!(middle.end_row, 665);
    // The slider is over the rows the editor shows.
    assert_close(middle.slider_top, middle.row_y(0) + 499.5 * 2.);
    assert_close(middle.slider_top, 270.);

    let end = layout(MinimapSize::Proportional, 1000, 999.);
    assert_close(end.top, 2058. - 600.);
    assert_eq!((end.first_row, end.end_row), (729, 1000));
    // The slider reaches the bottom as the editor reaches its last row.
    assert_close(end.slider_top, 540.);
    assert_close(end.slider_top + end.slider_height, 600.);
    assert_close(end.row_y(999), end.slider_top);

    // Scrolling past the last row stops there.
    assert_eq!(layout(MinimapSize::Proportional, 1000, 5000.), end);
}

#[test]
fn proportional_keeps_a_short_file_still() {
    for first in [0., 30.5, 99.] {
        let short = layout(MinimapSize::Proportional, 100, first);
        assert_eq!(short.row_height, 2.);
        assert_eq!(short.top, 0.);
        assert_eq!(
            (short.first_row, short.first_row_y, short.end_row),
            (0, 0., 100)
        );
        assert_close(short.slider_top, first * 2.);
        assert_eq!(short.slider_height, 60.);
    }
}

#[test]
fn fill_stretches_or_shrinks_the_rows_to_the_height() {
    // The rows and the space past the end fill the minimap, which never
    // scrolls.
    for (rows, row_height) in [(1000, 600. / 1029.), (100, 600. / 129.)] {
        for first in [0., (rows - 1) as f32 / 2., (rows - 1) as f32] {
            let fill = layout(MinimapSize::Fill, rows, first);
            assert_close(fill.row_height, row_height);
            assert_eq!(fill.top, 0.);
            assert_eq!((fill.first_row, fill.end_row), (0, rows));
            assert_close(fill.slider_height, 30. * row_height);
            assert_close(fill.slider_top, first * row_height);
        }
        let end = layout(MinimapSize::Fill, rows, (rows - 1) as f32);
        assert_close(end.slider_top + end.slider_height, 600.);
    }
    // Never taller than the editor's own rows.
    let one = layout(MinimapSize::Fill, 1, 0.);
    assert_close(one.row_height, 20.);
}

#[test]
fn fit_is_proportional_until_it_shrinks_as_fill() {
    for first in [0., 50., 99.] {
        let short = layout(MinimapSize::Fit, 100, first);
        assert_eq!(short, layout(MinimapSize::Proportional, 100, first));
    }
    for first in [0., 499.5, 999.] {
        let long = layout(MinimapSize::Fit, 1000, first);
        assert_eq!(long, layout(MinimapSize::Fill, 1000, first));
        assert_eq!(long.top, 0.);
        assert!(long.row_height < 2.);
    }
    // Just fitting at 2px rows: 271 rows and 29 past the end are 600px.
    let exact = layout(MinimapSize::Fit, 271, 270.);
    assert_eq!((exact.row_height, exact.top), (2., 0.));
    assert_close(exact.slider_top + exact.slider_height, 600.);
    assert!(layout(MinimapSize::Fit, 272, 0.).row_height < 2.);
}

#[test]
fn the_slider_is_at_least_a_row_tall() {
    let mut tiny = viewport(1000, 0.);
    tiny.visible_rows = 0.25;
    let layout = MinimapLayout::new(&MinimapOptions::default(), &tiny);
    assert_eq!(layout.slider_height, 2.);
}

#[test]
fn a_click_centres_the_row_under_it() {
    let middle = layout(MinimapSize::Proportional, 1000, 499.5);
    // top is 729: y 100 is at 829px, row 414.
    assert_eq!(middle.scroll_for_click(100.), 414);
    assert_eq!(middle.scroll_for_click(0.), 364);
    let short = layout(MinimapSize::Proportional, 100, 0.);
    assert_eq!(short.scroll_for_click(41.), 20);
    // Below the last row is the last row.
    assert_eq!(short.scroll_for_click(599.), 99);
    let fill = layout(MinimapSize::Fill, 100, 0.);
    assert_eq!(fill.scroll_for_click(10. * fill.row_height + 0.5), 10);
}

#[test]
fn dragging_the_slider_follows_the_pointer() {
    for size in [
        MinimapSize::Proportional,
        MinimapSize::Fill,
        MinimapSize::Fit,
    ] {
        for rows in [100, 1000] {
            let start = layout(size, rows, 10.);
            for dy in [1., 37.5, 150.] {
                let first = start.scroll_for_drag(10., dy);
                let moved = layout(size, rows, first);
                assert_close(moved.slider_top, start.slider_top + dy);
                // And back.
                assert_close(moved.scroll_for_drag(first, -dy), 10.);
            }
            // Dragging the slider to the bottom reaches the last page, the
            // last row at the top, and past the top the first.
            let to_bottom = 600. - start.slider_height - start.slider_top;
            let last = start.scroll_for_drag(10., to_bottom + 50.);
            assert_eq!(last, (rows - 1) as f32, "{size:?} {rows}");
            let end = layout(size, rows, last);
            assert_close(end.slider_top, end.row_y(rows - 1));
            assert_eq!(start.scroll_for_drag(10., -1000.), 0.);
        }
    }
    // A file of one row doesn't scroll.
    let one = layout(MinimapSize::Proportional, 1, 0.);
    assert_eq!(one.scroll_for_drag(0., 100.), 0.);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Default,
    Keyword,
    Name,
}
use Ink::*;

fn runs(row: &str, spans: &[(Range<usize>, Ink)], max_column: u32) -> Vec<(Range<u32>, Ink)> {
    row_runs(row, 4, spans, Default, max_column)
        .into_iter()
        .map(|run| (run.columns, run.color))
        .collect()
}

#[test]
fn runs_split_at_whitespace_and_colour_changes() {
    let spans = [(0..2, Keyword), (3..4, Name)];
    assert_eq!(
        runs("fn a() {}", &spans, 120),
        [
            (0..2, Keyword),
            (3..4, Name),
            (4..6, Default),
            (7..9, Default)
        ]
    );
    // One colour, split by spaces only.
    assert_eq!(runs("ab  cd", &[], 120), [(0..2, Default), (4..6, Default)]);
    assert_eq!(runs("   ", &[], 120), []);
    assert_eq!(runs("", &[], 120), []);
}

#[test]
fn runs_merge_adjacent_spans_of_one_colour() {
    let spans = [(0..1, Name), (1..2, Name), (2..3, Keyword)];
    assert_eq!(runs("abc", &spans, 120), [(0..2, Name), (2..3, Keyword)]);
    // Unspanned bytes take the default and merge with a default span.
    let spans = [(0..1, Default)];
    assert_eq!(runs("abc", &spans, 120), [(0..3, Default)]);
    // Overlapping spans: the first wins.
    let spans = [(0..3, Keyword), (1..2, Name)];
    assert_eq!(runs("abc", &spans, 120), [(0..3, Keyword)]);
}

#[test]
fn runs_expand_tabs_and_count_wide_chars_twice() {
    // Tabs reach the next multiple of 4.
    assert_eq!(runs("\tx\ty", &[], 120), [(4..5, Default), (8..9, Default)]);
    assert_eq!(runs("ab\tc", &[], 120), [(0..2, Default), (4..5, Default)]);
    // A wide char is two columns; tab stops count it as one char, as
    // ExpandedRow and VS Code do.
    let spans = [(0..6, Name)];
    assert_eq!(runs("日本x", &spans, 120), [(0..4, Name), (4..5, Default)]);
    assert_eq!(runs("日\tx", &[], 120), [(0..2, Default), (5..6, Default)]);
    assert_eq!(runs("ａb", &[], 120), [(0..3, Default)]);
    // Fullwidth symbols, U+FFE0 to U+FFE6, too: `￥` is two columns and
    // three bytes, so the colour change after it is at column 2.
    for symbol in ['\u{FFE0}', '￥', '\u{FFE6}'] {
        let row = format!("{symbol}x");
        let spans = [(0..3, Name), (3..4, Keyword)];
        assert_eq!(
            runs(&row, &spans, 120),
            [(0..2, Name), (2..3, Keyword)],
            "{row}"
        );
        assert_eq!(runs(&row, &spans, 1), [(0..1, Name)], "{row}");
        assert_eq!(runs(&row, &spans, 2), [(0..2, Name)], "{row}");
    }
    assert_eq!(runs("\u{FFE7}x", &[], 120), [(0..2, Default)]);
    assert_eq!(runs("😀 é", &[], 120), [(0..2, Default), (3..4, Default)]);
    // A wide space is a gap two columns wide.
    assert_eq!(
        runs("a\u{3000}b", &[], 120),
        [(0..1, Default), (3..4, Default)]
    );
    // Spans are bytes: `é` is two.
    let spans = [(0..2, Keyword), (2..3, Name)];
    assert_eq!(runs("éa", &spans, 120), [(0..1, Keyword), (1..2, Name)]);
}

#[test]
fn runs_clip_at_max_column() {
    assert_eq!(runs("abcdef", &[], 3), [(0..3, Default)]);
    assert_eq!(runs("ab cdef", &[], 4), [(0..2, Default), (3..4, Default)]);
    // A wide char straddling the edge is cut there.
    assert_eq!(runs("a日b", &[], 2), [(0..2, Default)]);
    // A tab past the edge ends the row.
    assert_eq!(runs("a\tb", &[], 4), [(0..1, Default)]);
    // Long rows stop at the edge.
    let long = "x".repeat(10_000);
    assert_eq!(runs(&long, &[], 120), [(0..120, Default)]);
}

#[test]
fn defaults_match_vscode() {
    let options = MinimapOptions::default();
    assert_eq!(options.side, MinimapSide::Right);
    assert_eq!((options.scale, options.max_column), (1, 120));
    assert_eq!(options.size, MinimapSize::Proportional);
    assert_eq!(options.show_slider, MinimapShowSlider::Mouseover);
    assert!(options.render_characters);
}
