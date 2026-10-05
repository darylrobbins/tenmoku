//! Clicking the gutter's fold chevrons and the markers after folded rows, in
//! a headless window.

use fe_buffer::Buffer;
use fe_display::layout::Rect;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext};

use crate::*;

const TEXT: &str = "fn f() {\n    one();\n    two();\n}\nfn g() {}\n";

/// A focused Rust editor on [`TEXT`] with the caret on its last function,
/// drawn once its tree is parsed.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), Some("main.rs".into()));
        model.set_cursor(TEXT.find("fn g").unwrap());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    let parsed = view.update(cx, |view, _| {
        view.syntax().and_then(fe_syntax::Syntax::tree).is_some()
    });
    assert!(parsed);
    (view, cx)
}

/// The window position of the middle of `rect` in the last frame.
fn center_of(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    rect: impl FnOnce(&fe_display::layout::Frame<crate::element::Line>) -> Rect,
) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(rect(&layout.frame)).center()
    })
}

/// The folds as `(header, last)` rows, and the caret.
fn folds_and_head(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Vec<(u32, u32)>, usize) {
    view.update(cx, |view, _| {
        let model = view.model();
        let folds = model.display().folds();
        let folds = folds.iter().map(|fold| (fold.start, fold.end)).collect();
        (folds, model.primary_selection().head())
    })
}

#[gpui::test]
fn clicking_a_chevron_toggles_its_fold(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let head = TEXT.find("fn g").unwrap();
    let toggles = view.update(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        let toggles = frame.fold_toggles.iter();
        toggles
            .map(|toggle| (toggle.buffer_row, toggle.folded))
            .collect::<Vec<_>>()
    });
    assert_eq!(toggles, [(0, false)]);
    let chevron = center_of(&view, cx, |frame| frame.fold_toggles[0].rect);

    cx.simulate_click(chevron, Modifiers::none());
    assert_eq!(folds_and_head(&view, cx), (vec![(0, 2)], head));
    let folded = view.update(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        frame.fold_toggles[0].folded
    });
    assert!(folded, "the chevron shows the row is folded");

    // The chevron stays put, so the same spot unfolds it.
    assert_eq!(
        center_of(&view, cx, |frame| frame.fold_toggles[0].rect),
        chevron
    );
    cx.simulate_click(chevron, Modifiers::none());
    assert_eq!(folds_and_head(&view, cx), (vec![], head));
}

#[gpui::test]
fn clicking_the_marker_unfolds(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let head = TEXT.find("fn g").unwrap();
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(0..=2);
        cx.notify();
    });
    cx.run_until_parked();
    let marker = center_of(&view, cx, |frame| frame.fold_markers[0].rect);

    cx.simulate_click(marker, Modifiers::none());
    assert_eq!(folds_and_head(&view, cx), (vec![], head));
    let markers = view.update(cx, |view, _| {
        view.layout
            .as_ref()
            .expect("drawn")
            .frame
            .fold_markers
            .len()
    });
    assert_eq!(markers, 0);
}

#[gpui::test]
fn clicking_keeps_the_selections_a_fold_doesnt_hide(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let selection =
        |range: std::ops::Range<usize>, reversed| fe_buffer::ResolvedSelection { range, reversed };
    let header_end = TEXT.find('\n').unwrap();
    let fn_g = TEXT.find("fn g").unwrap();
    // Text on the header, a caret in the rows the fold hides, and a
    // reversed selection below it.
    let before = [
        selection(0..2, false),
        selection(TEXT.find("one").unwrap()..TEXT.find("one").unwrap(), false),
        selection(fn_g..fn_g + 4, true),
    ];
    view.update(cx, |view, cx| {
        view.model_mut().set_selections(before.to_vec());
        cx.notify();
    });
    cx.run_until_parked();
    let selections =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model().selections());
    let chevron = center_of(&view, cx, |frame| frame.fold_toggles[0].rect);

    // The hidden caret moves to the end of the header, as folding with the
    // keyboard moves it; the others stay.
    cx.simulate_click(chevron, Modifiers::none());
    let folded = [
        before[0].clone(),
        selection(header_end..header_end, false),
        before[2].clone(),
    ];
    assert_eq!(selections(cx), folded);
    cx.simulate_click(chevron, Modifiers::none());
    assert_eq!(selections(cx), folded);
}
