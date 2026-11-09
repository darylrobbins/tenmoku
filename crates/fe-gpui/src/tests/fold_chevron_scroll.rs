//! Clicking a fold chevron leaves the view's scroll and the fold's header
//! where they were, instead of scrolling to the caret.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext};

use crate::{EditorTheme, EditorView, init};

/// 60 functions of 4 rows each: 240 rows.
fn text() -> String {
    "fn f() {\n    a();\n    b();\n}\n".repeat(60)
}

/// A focused Rust editor on [`text`] scrolled to row 100, with `prepare`
/// run on its model first, drawn once its tree is parsed.
fn editor(
    cx: &mut TestAppContext,
    prepare: impl FnOnce(&mut EditorModel) + 'static,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text()), Some("main.rs".into()));
        prepare(&mut model);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    let parsed = view.update(cx, |view, _| {
        view.syntax().and_then(fe_syntax::Syntax::tree).is_some()
    });
    assert!(parsed);
    view.update(cx, |view, cx| {
        view.set_scroll_top(100., cx);
    });
    cx.run_until_parked();
    (view, cx)
}

/// The centre of row 108's chevron in the last frame, and whether it's folded.
fn chevron(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Point<Pixels>, bool) {
    view.update(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let toggle = layout
            .frame
            .fold_toggles
            .iter()
            .find(|toggle| toggle.buffer_row == 108)
            .expect("row 108 has a chevron");
        (layout.bounds(toggle.rect).center(), toggle.folded)
    })
}

fn folds(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, u32)> {
    view.update(cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    })
}

fn scroll_top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.update(cx, |view, _| view.scroll_top())
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.update(cx, |view, _| view.model().primary_selection().head())
}

#[gpui::test]
fn collapsing_by_its_chevron_keeps_the_view_where_it_is(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, |_| {});
    let top = scroll_top(&view, cx);
    let (at, folded) = chevron(&view, cx);
    assert!(!folded);

    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(folds(&view, cx), [(108, 110)]);
    assert_eq!(scroll_top(&view, cx), top);
    assert_eq!(chevron(&view, cx), (at, true));
    assert_eq!(head(&view, cx), 0);
}

#[gpui::test]
fn expanding_by_its_chevron_keeps_the_view_where_it_is(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, |model| model.fold_rows(108..=110));
    let top = scroll_top(&view, cx);
    let (at, folded) = chevron(&view, cx);
    assert!(folded);
    assert_eq!(folds(&view, cx), [(108, 110)]);

    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(folds(&view, cx), []);
    assert_eq!(scroll_top(&view, cx), top);
    assert_eq!(chevron(&view, cx), (at, false));
    assert_eq!(head(&view, cx), 0);
}

#[gpui::test]
fn collapsing_over_the_caret_moves_it_to_the_header_in_place(cx: &mut TestAppContext) {
    let text = text();
    let row = |n: usize| {
        text.split_inclusive('\n')
            .take(n)
            .map(str::len)
            .sum::<usize>()
    };
    let (start, end) = (row(109), row(109) - 1);
    let (view, cx) = editor(cx, move |model| model.set_cursor(start));
    let top = scroll_top(&view, cx);
    let (at, _) = chevron(&view, cx);

    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(folds(&view, cx), [(108, 110)]);
    assert_eq!(head(&view, cx), end);
    assert_eq!(scroll_top(&view, cx), top);
}
