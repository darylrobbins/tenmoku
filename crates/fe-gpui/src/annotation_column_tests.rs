//! The host's annotation column left of the line numbers
//! ([`crate::gutter_annotations`]), in a headless window.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use gpui::{
    Entity, Focusable as _, Hsla, Modifiers, Pixels, Point, SharedString, TestAppContext,
    VisualTestContext,
};

use crate::{hover::HOVER_DELAY, *};

const TEXT: &str = "zero\none\ntwo\nthree\nfour\nfive\nsix\n";
const NEW: &str = "2026-10-11 Ann Example";
const OLD: &str = "2025-03-02 Bob";

/// A focused editor on [`TEXT`] in an active window, its caret at the
/// start.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(0);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

/// The entries `(first row..end row, text, heat)` anchored at the buffer's
/// version, popovers saying which entry and row they're for, and clicks
/// recorded in `clicks`.
fn annotate(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    entries: &[(std::ops::Range<u32>, &str, Option<f32>)],
    clicks: &Rc<RefCell<Vec<(usize, u32)>>>,
) {
    let clicks = clicks.clone();
    view.update(cx, |view, cx| {
        let snapshot = view.model().snapshot().clone();
        let text = snapshot.text();
        let entries = (entries.iter())
            .map(|(rows, label, heat)| GutterAnnotation {
                rows: snapshot.anchor_before(text.row_start(rows.start))
                    ..snapshot.anchor_before(text.row_start(rows.end)),
                text: SharedString::from(label.to_string()),
                heat: *heat,
            })
            .collect();
        let annotations = GutterAnnotations {
            version: snapshot.version(),
            entries,
            hover: Rc::new(|ix, row| {
                Some(HoverContent {
                    text: format!("entry {ix} at row {row}"),
                    range: None,
                })
            }),
            on_click: Rc::new(move |ix, row, _, _| clicks.borrow_mut().push((ix, row))),
        };
        view.set_gutter_annotations(Some(annotations), cx);
    });
    cx.run_until_parked();
}

/// Rows 0–2 by [`NEW`], hot; row 4 by [`OLD`], cold; row 3 in none.
fn annotate_two(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<(usize, u32)>>> {
    let clicks = Rc::default();
    annotate(
        view,
        cx,
        &[(0..3, NEW, Some(1.0)), (4..5, OLD, Some(0.0))],
        &clicks,
    );
    clicks
}

type Painted = Vec<(u32, Option<SharedString>, Option<Hsla>)>;

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Painted {
    view.read_with(cx, |view, _| view.painted_annotations.clone())
}

/// The tint for `heat`.
fn tint(view: &Entity<EditorView>, cx: &mut VisualTestContext, heat: f32) -> Hsla {
    view.read_with(cx, |view, _| {
        (view.theme.gutter_active_foreground).opacity(0.04 + 0.16 * heat)
    })
}

/// The width of `text` in the editor's font, and its em.
fn shaped(view: &Entity<EditorView>, cx: &mut VisualTestContext, text: &str) -> (f32, f32) {
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    cx.update(|window, _| {
        let font = gpui::font(theme.font_family.clone());
        let text_system = window.text_system();
        let id = text_system.resolve_font(&font);
        let em = text_system.em_advance(id, theme.font_size).unwrap();
        let run = gpui::TextRun {
            len: text.len(),
            font,
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = text_system.layout_line(text, theme.font_size, &[run], None);
        (line.width.into(), em.into())
    })
}

/// The last frame's annotation column width and text left.
fn column(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (f32, f32) {
    view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        (frame.annotation_column.width, frame.text_left)
    })
}

fn slot(view: &Entity<EditorView>, cx: &mut VisualTestContext, row: u32) -> Point<Pixels> {
    view.read_with(cx, |view, _| view.annotation_bounds(row))
        .expect("the row is in the frame")
        .center()
}

fn rest(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.run_until_parked();
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
}

#[gpui::test]
fn entries_paint_once_per_run_tinted_by_heat(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (_, text_left) = column(&view, cx);
    annotate_two(&view, cx);
    let (hot, cold) = (tint(&view, cx, 1.0), tint(&view, cx, 0.0));
    let text = |label: &str| Some(SharedString::from(label.to_string()));
    let rows = painted(&view, cx);
    assert_eq!(
        rows[..7],
        [
            (0, text(NEW), Some(hot)),
            (1, None, Some(hot)),
            (2, None, Some(hot)),
            (3, None, None),
            (4, text(OLD), Some(cold)),
            (5, None, None),
            (6, None, None),
        ]
    );
    assert!(rows[7..].iter().all(|row| *row == (row.0, None, None)));
    assert!(hot.a > cold.a, "{hot:?} vs {cold:?}");

    // The widest entry's text and an em, before the gutter.
    let (widest, em) = shaped(&view, cx, NEW);
    let (width, left) = column(&view, cx);
    assert_eq!(width, widest + em);
    assert_eq!(left, text_left + width);

    // At most 32 em.
    let long = "x".repeat(200);
    annotate(&view, cx, &[(0..1, &long, None)], &Rc::default());
    assert!(shaped(&view, cx, &long).0 > 32. * em);
    assert_eq!(column(&view, cx).0, 32. * em);
    assert_eq!(painted(&view, cx)[0], (0, text(&long), None));
}

#[gpui::test]
fn hovering_and_clicking_an_entry(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let clicks = annotate_two(&view, cx);
    let shown = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.popover_text());

    rest(slot(&view, cx, 1), cx);
    assert_eq!(shown(cx).as_deref(), Some("entry 0 at row 1"));
    // Within the entry it stays; into the text, it goes.
    rest(slot(&view, cx, 2), cx);
    assert_eq!(shown(cx).as_deref(), Some("entry 0 at row 1"));
    let text = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(layout.frame.text_bounds()).center()
    });
    rest(text, cx);
    assert_eq!(shown(cx), None);
    // Nothing over a row in no entry.
    rest(slot(&view, cx, 3), cx);
    assert_eq!(shown(cx), None);

    let state = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let model = view.model();
            let folds: Vec<_> = (model.display().folds().iter())
                .map(|fold| (fold.start, fold.end))
                .collect();
            (model.selections(), folds)
        })
    };
    let before = state(cx);
    assert_eq!(
        before.0,
        [ResolvedSelection {
            range: 0..0,
            reversed: false
        }]
    );
    let row_4 = slot(&view, cx, 4);
    cx.simulate_click(row_4, Modifiers::none());
    assert_eq!(*clicks.borrow(), [(1, 4)]);
    assert_eq!(state(cx), before);

    let row_3 = slot(&view, cx, 3);
    cx.simulate_click(row_3, Modifiers::none());
    assert_eq!(*clicks.borrow(), [(1, 4)]);
    assert_eq!(state(cx), before);
}

#[gpui::test]
fn hiding_the_gutter_hides_an_entrys_popover(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    annotate_two(&view, cx);
    let shown = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.popover_text());
    let set_gutter = |shown: bool, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| view.set_gutter(shown, cx));
        cx.run_until_parked();
    };

    // Shown, then the gutter hidden with the pointer still.
    let row_1 = slot(&view, cx, 1);
    rest(row_1, cx);
    assert_eq!(shown(cx).as_deref(), Some("entry 0 at row 1"));
    set_gutter(false, cx);
    assert_eq!(shown(cx), None);
    assert_eq!(column(&view, cx).0, 0.);

    // Hidden while waiting out the delay: it never shows, not even once
    // the gutter is back.
    set_gutter(true, cx);
    let row_0 = slot(&view, cx, 0);
    cx.simulate_mouse_move(row_0, None, Modifiers::none());
    cx.run_until_parked();
    set_gutter(false, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(shown(cx), None);
    set_gutter(true, cx);
    assert_eq!(shown(cx), None);
}

#[gpui::test]
fn hiding_and_showing_the_gutter_before_the_delay_cancels_the_wait(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    annotate_two(&view, cx);
    let row_0 = slot(&view, cx, 0);
    cx.simulate_mouse_move(row_0, None, Modifiers::none());
    cx.run_until_parked();
    for shown in [false, true] {
        view.update(cx, |view, cx| view.set_gutter(shown, cx));
        cx.run_until_parked();
    }
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.popover_text()), None);
}

#[gpui::test]
fn edited_rows_go_blank_and_later_rows_follow(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (_, text_left) = column(&view, cx);
    annotate_two(&view, cx);
    let (hot, cold) = (tint(&view, cx, 1.0), tint(&view, cx, 0.0));
    view.update(cx, |view, cx| {
        let start = view.model().text().row_start(1);
        view.model_mut().set_cursor(start);
        view.model_mut().insert("new\n");
        cx.notify();
    });
    cx.run_until_parked();
    let text = |label: &str| Some(SharedString::from(label.to_string()));
    assert_eq!(
        painted(&view, cx)[..7],
        [
            (0, text(NEW), Some(hot)),
            (1, None, None),
            (2, None, Some(hot)),
            (3, None, Some(hot)),
            (4, None, None),
            (5, text(OLD), Some(cold)),
            (6, None, None),
        ]
    );

    view.update(cx, |view, cx| view.set_gutter_annotations(None, cx));
    cx.run_until_parked();
    assert_eq!(column(&view, cx), (0., text_left));
    assert!(painted(&view, cx).is_empty());
}
