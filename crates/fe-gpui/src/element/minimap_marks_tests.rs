//! The minimap's marks in a headless window: selections, find matches,
//! errors and warnings at their rows and columns, git's changes in a lane,
//! only on the rows shown, and none of them rebuilding a tile.

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::minimap::{MinimapOptions, MinimapSize};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, Severity},
};
use fe_vim::search::{FindQuery, Pattern};
use gpui::{
    Bounds, Entity, Focusable as _, Hsla, Pixels, TestAppContext, VisualTestContext, point, px,
    size,
};

use super::LANE_WIDTH;
use crate::{EditorTheme, EditorView, element::minimap::Painted, init};

/// An editor on `text` as the file at `path`, in an 800 × 400 window, with
/// the minimap on, drawn.
fn editor<'a>(
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
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    view.update(cx, |view, cx| {
        view.set_minimap(Some(MinimapOptions::default()), cx)
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    frame(cx);
}

/// Draws a frame without letting background work land first.
fn frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Painted {
    view.read_with(cx, |view, _| view.painted_minimap.clone())
        .expect("shown")
}

fn theme(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> EditorTheme {
    view.read_with(cx, |view, _| view.theme.clone())
}

/// The marks under the tiles in `color`, as (display row, columns): a row
/// is 2 pixels tall, a column a pixel wide.
fn marks(
    view: &Entity<EditorView>,
    color: Hsla,
    cx: &mut VisualTestContext,
) -> Vec<(u32, Range<u32>)> {
    let painted = painted(view, cx);
    let origin = painted.bounds.origin;
    (painted.marks.iter())
        .filter(|(_, mark)| *mark == color)
        .map(|(bounds, _)| {
            assert_eq!(bounds.size.height, px(2.), "{bounds:?}");
            let row = f32::from(bounds.top() - origin.y) / 2.;
            let left = f32::from(bounds.left() - origin.x) as u32;
            (row as u32, left..left + f32::from(bounds.size.width) as u32)
        })
        .collect()
}

/// The tiles built since the last call.
fn built(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        std::mem::take(&mut view.minimap_cache.borrow_mut().built)
    })
}

fn select(view: &Entity<EditorView>, ranges: &[Range<usize>], cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.model
            .set_selections(ranges.iter().map(|range| ResolvedSelection {
                range: range.clone(),
                reversed: false,
            }));
        cx.notify();
    });
    draw(cx);
}

fn select_one(view: &Entity<EditorView>, range: Range<usize>, cx: &mut VisualTestContext) {
    select(view, &[range], cx);
}

fn find(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    let pattern = Pattern::from_query(&FindQuery {
        text: text.into(),
        match_case: true,
        ..FindQuery::default()
    })
    .unwrap();
    view.update(cx, |view, cx| view.set_find(Some(pattern), cx));
}

fn diagnose(
    view: &Entity<EditorView>,
    items: Vec<(Range<usize>, Severity)>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let items = (items.into_iter())
            .map(|(range, severity)| (range, Diagnostic::new(severity, "marked")))
            .collect();
        assert!(view.model_mut().apply_diagnostics(
            1u32,
            (1u64, 1u64),
            version,
            items,
            Some((1, 1))
        ));
        cx.notify();
    });
}

/// `rows` numbered rows.
fn numbered(rows: usize) -> String {
    (0..rows)
        .map(|n| format!("let value_{n} = {n};\n"))
        .collect()
}

const TEXT: &str = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n\tlet z = 3;\n";

fn at(text: &str, needle: &str) -> usize {
    text.find(needle).expect("in the text")
}

#[gpui::test]
fn selections_mark_their_columns_and_the_rows_between_to_their_ends(cx: &mut TestAppContext) {
    let (view, cx) = editor(TEXT, "main.rs", cx);
    let color = theme(&view, cx).minimap_selection;
    let z = at(TEXT, "z");
    // From `main` into row 2, a caret on row 3, and `z` after a tab.
    select(
        &view,
        &[
            at(TEXT, "main")..at(TEXT, " = 2"),
            at(TEXT, "}\n\t")..at(TEXT, "}\n\t"),
            z..z + 1,
        ],
        cx,
    );
    assert_eq!(
        marks(&view, color, cx),
        [(0, 3..11), (1, 0..14), (2, 0..9), (4, 8..9)]
    );

    // Carets alone mark nothing.
    select(&view, &[0..0, z..z], cx);
    assert_eq!(marks(&view, color, cx), []);
}

#[gpui::test]
fn counted_find_matches_mark_their_columns_and_a_stale_count_nothing(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(5), "main.rs", cx);
    let color = theme(&view, cx).minimap_find_match;
    find(&view, "value", cx);
    draw(cx);
    let all: Vec<_> = (0..5).map(|row| (row, 4..9)).collect();
    assert_eq!(marks(&view, color, cx), all);

    // Edited, the count is of another text until the next lands.
    view.update(cx, |view, cx| {
        view.model.set_selections([ResolvedSelection {
            range: 0..0,
            reversed: false,
        }]);
        view.model.insert("// ");
        cx.notify();
    });
    frame(cx);
    assert_eq!(marks(&view, color, cx), []);
    draw(cx);
    let moved: Vec<_> = (0..5)
        .map(|row| (row, if row == 0 { 7..12 } else { 4..9 }))
        .collect();
    assert_eq!(marks(&view, color, cx), moved);
}

#[gpui::test]
fn errors_and_warnings_mark_their_columns_and_information_and_hints_nothing(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(TEXT, "main.rs", cx);
    let theme = theme(&view, cx);
    let x = at(TEXT, "x");
    let main_ = at(TEXT, "main");
    diagnose(
        &view,
        vec![
            (x..x + 1, Severity::Error),
            // Zero-width: a column.
            (main_..main_, Severity::Error),
            (at(TEXT, "y")..at(TEXT, "2;"), Severity::Warning),
            (at(TEXT, "1;")..at(TEXT, "1;") + 1, Severity::Information),
            (at(TEXT, "2;")..at(TEXT, "2;") + 1, Severity::Hint),
        ],
        cx,
    );
    draw(cx);
    assert_eq!(
        marks(&view, theme.minimap_error, cx),
        [(0, 3..4), (1, 8..9)]
    );
    assert_eq!(marks(&view, theme.minimap_warning, cx), [(2, 8..12)]);
    let all = painted(&view, cx).marks;
    assert_eq!(all.len(), 3, "{all:?}");
}

#[gpui::test]
fn git_changes_mark_a_lane_at_the_inner_edge(cx: &mut TestAppContext) {
    let base = "a\nb\nc\nd\ne\nf\n";
    // `b` modified, `new` added after `c`, `e` deleted.
    let (view, cx) = editor("a\nB\nc\nnew\nd\nf\n", "notes.txt", cx);
    view.update(cx, |view, cx| {
        view.model.set_diff_base(Some(base.into()));
        cx.notify();
    });
    draw(cx);
    let theme = theme(&view, cx);
    let painted = painted(&view, cx);
    let (left, top) = (painted.bounds.left(), painted.bounds.top());
    let bar = |y: f32, height: f32| {
        Bounds::new(point(left, top + px(y)), size(px(LANE_WIDTH), px(height)))
    };
    let lane: Vec<(Bounds<Pixels>, Hsla)> = vec![
        (bar(2., 2.), theme.minimap_gutter_modified),
        (bar(6., 2.), theme.minimap_gutter_added),
        // Between `d` and `f`, a row's height across the edge.
        (bar(9., 2.), theme.minimap_gutter_deleted),
    ];
    assert_eq!(painted.lane, lane);
    assert_eq!(painted.marks, [], "the lane's marks aren't under the tiles");
}

#[gpui::test]
fn rows_deleted_from_the_end_mark_the_edge_below_the_last_row(cx: &mut TestAppContext) {
    let (view, cx) = editor("a", "notes.txt", cx);
    view.update(cx, |view, cx| {
        view.model.set_diff_base(Some("a\nb".into()));
        cx.notify();
    });
    draw(cx);
    let theme = theme(&view, cx);
    let painted = painted(&view, cx);
    let (left, top) = (painted.bounds.left(), painted.bounds.top());
    // A row's height across the edge below row 0.
    let mark = Bounds::new(point(left, top + px(1.)), size(px(LANE_WIDTH), px(2.)));
    assert_eq!(painted.lane, [(mark, theme.minimap_gutter_deleted)]);
}

#[gpui::test]
fn only_the_counted_matches_on_the_rows_shown_are_marked(cx: &mut TestAppContext) {
    // A match on every row, 100,000 of them: 10,000 are counted.
    let (view, cx) = editor(&numbered(100_000), "notes.txt", cx);
    let color = theme(&view, cx).minimap_find_match;
    find(&view, "value", cx);
    draw(cx);
    let shown = view.read_with(cx, |view, _| {
        let geometry = &view
            .layout
            .as_ref()
            .expect("drawn")
            .minimap
            .as_ref()
            .expect("shown")
            .geometry;
        geometry.first_row..geometry.end_row
    });
    assert_eq!(shown, 0..200, "the minimap's 400 pixels");
    let all: Vec<_> = shown.map(|row| (row, 4..9)).collect();
    assert_eq!(marks(&view, color, cx), all);

    // Past the count, nothing; a selection off the rows shown isn't built.
    let far = view.read_with(cx, |view, _| view.model.text().row_start(90_000));
    select_one(&view, far..far + 3, cx);
    assert_eq!(painted(&view, cx).marks.len(), 200);
    view.update(cx, |view, cx| view.set_scroll_top(50_000., cx));
    draw(cx);
    assert_eq!(painted(&view, cx).marks, []);
}

#[gpui::test]
fn a_caret_move_a_find_count_or_diagnostics_rebuild_no_tile(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), "main.rs", cx);
    assert!(!built(&view, cx).is_empty());
    let theme = theme(&view, cx);

    select_one(&view, 30..60, cx);
    assert!(!marks(&view, theme.minimap_selection, cx).is_empty());
    find(&view, "value", cx);
    draw(cx);
    assert!(!marks(&view, theme.minimap_find_match, cx).is_empty());
    diagnose(&view, vec![(4..9, Severity::Error)], cx);
    draw(cx);
    assert_eq!(marks(&view, theme.minimap_error, cx), [(0, 4..9)]);
    select_one(&view, 70..70, cx);
    assert_eq!(built(&view, cx), Vec::<u32>::new());
}

#[gpui::test]
fn a_whole_file_selection_filling_the_minimap_marks_only_the_rows_the_tiles_draw(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(&numbered(100_000), "notes.txt", cx);
    let fill = MinimapOptions {
        size: MinimapSize::Fill,
        ..MinimapOptions::default()
    };
    view.update(cx, |view, cx| view.set_minimap(Some(fill), cx));
    let end = view.read_with(cx, |view, _| view.model.text().len());
    select_one(&view, 0..end, cx);
    let scale = cx.update(|window, _| window.scale_factor());
    let painted = painted(&view, cx);
    // A row drawn a device pixel tall at least: as many as the minimap's
    // device pixels, and the selection's last row.
    let device_rows = f32::from(painted.bounds.size.height) * scale;
    assert!(
        painted.marks.len() as f32 <= device_rows + 1.,
        "{} marks",
        painted.marks.len()
    );
    assert!(painted.marks.len() > 100, "{} marks", painted.marks.len());
    for (bounds, _) in &painted.marks {
        assert!(f32::from(bounds.size.height) * scale >= 1., "{bounds:?}");
    }
}
