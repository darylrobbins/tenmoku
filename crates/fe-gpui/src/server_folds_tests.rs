//! A host's folding ranges: they replace the syntax's candidates for fold,
//! fold all, the chevrons and a click on one, are sanitised as VS Code does,
//! follow edits, and give a language without a grammar folds.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::sanitize;
use crate::commands::RunCommand;
use crate::*;

/// Two functions: the syntax folds rows 0..=2 and 4..=5.
const TEXT: &str = "fn f() {\n    one();\n    two();\n}\nfn g() {\n    three();\n}\n";

/// A focused editor on [`TEXT`] named `name`, drawn (and parsed, if the
/// name has a grammar).
fn editor<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let name = name.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), Some(name.into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn set(view: &Entity<EditorView>, cx: &mut VisualTestContext, ranges: Option<Vec<(u32, u32)>>) {
    view.update(cx, |view, cx| {
        view.set_fold_ranges(ranges.map(|r| r.into_iter().map(|(a, b)| a..=b).collect()));
        cx.notify();
    });
    draw(cx);
}

fn run(view: &Entity<EditorView>, cx: &mut VisualTestContext, command: &str) {
    let invocation = Invocation::of(command).unwrap();
    cx.update(|window, cx| {
        assert!(view.focus_handle(cx).is_focused(window));
        window.dispatch_action(Box::new(RunCommand(invocation)), cx);
    });
    draw(cx);
}

/// The rows with a chevron in the last frame.
fn chevrons(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.update(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        frame.fold_toggles.iter().map(|t| t.buffer_row).collect()
    })
}

/// The folds as `(header, last)` rows.
fn folds(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, u32)> {
    view.update(cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    })
}

#[gpui::test]
fn server_ranges_replace_the_syntax_candidates(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "main.rs");
    assert_eq!(chevrons(&view, cx), [0, 4]);

    // Rows the syntax wouldn't fold, and none of the rows it would.
    set(&view, cx, Some(vec![(1, 3)]));
    assert_eq!(chevrons(&view, cx), [1]);

    let chevron = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(layout.frame.fold_toggles[0].rect).center()
    });
    cx.simulate_click(chevron, Modifiers::none());
    assert_eq!(folds(&view, cx), [(1, 3)]);
    cx.simulate_click(chevron, Modifiers::none());
    assert_eq!(folds(&view, cx), []);
}

#[gpui::test]
fn overlapping_ranges_are_sanitised(_: &mut TestAppContext) {
    let ranges = vec![
        0..=5,
        // The same start: the widest stays.
        0..=3,
        // Starts inside 0..=5 and ends after it.
        2..=7,
        // One row.
        1..=1,
        3..=4,
        // Inside 0..=5, but ends after 3..=4.
        4..=5,
        // Alone, and nested with a last row of its parent's.
        8..=12,
        9..=12,
        // Backwards.
        std::ops::RangeInclusive::new(20, 15),
    ];
    assert_eq!(sanitize(ranges), [0..=5, 3..=4, 8..=12, 9..=12]);
}

#[gpui::test]
fn fold_all_and_fold_at_use_the_server_ranges(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "main.rs");
    set(&view, cx, Some(vec![(1, 3), (4, 6)]));

    run(&view, cx, "editor.fold_all");
    assert_eq!(folds(&view, cx), [(1, 3), (4, 6)]);
    run(&view, cx, "editor.unfold_all");
    assert_eq!(folds(&view, cx), []);

    // The caret inside a range folds the range containing it.
    view.update(cx, |view, _| {
        let at = TEXT.find("two").unwrap();
        view.model_mut().set_cursor(at);
    });
    run(&view, cx, "editor.fold");
    assert_eq!(folds(&view, cx), [(1, 3)]);
    run(&view, cx, "editor.unfold_all");
}

#[gpui::test]
fn ranges_follow_edits_above_them(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "main.rs");
    set(&view, cx, Some(vec![(1, 3), (4, 6)]));

    view.update(cx, |view, _| {
        view.model_mut().set_cursor(0);
        view.model_mut().insert("// a\n// b\n");
    });
    draw(cx);
    assert_eq!(chevrons(&view, cx), [3, 6]);
    run(&view, cx, "editor.fold_all");
    assert_eq!(folds(&view, cx), [(3, 5), (6, 8)]);
    run(&view, cx, "editor.unfold_all");

    // An edit across the start of a header's row leaves its range nothing
    // to anchor to.
    view.update(cx, |view, _| {
        let model = view.model_mut();
        let from = model.text().row_start(5) + 1;
        let to = model.text().row_start(6) + 2;
        model.select_range(from..to, false);
        model.insert("x");
    });
    draw(cx);
    assert_eq!(chevrons(&view, cx), [3]);
}

#[gpui::test]
fn none_restores_the_syntax(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "main.rs");
    set(&view, cx, Some(vec![(1, 3)]));
    assert_eq!(chevrons(&view, cx), [1]);
    set(&view, cx, None);
    assert_eq!(chevrons(&view, cx), [0, 4]);
    run(&view, cx, "editor.fold_all");
    assert_eq!(folds(&view, cx), [(0, 2), (4, 5)]);
}

#[gpui::test]
fn a_language_without_a_grammar_folds(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "notes.nogrammar");
    assert_eq!(chevrons(&view, cx), Vec::<u32>::new());

    set(&view, cx, Some(vec![(0, 2), (4, 5)]));
    assert_eq!(chevrons(&view, cx), [0, 4]);
    run(&view, cx, "editor.fold_all");
    assert_eq!(folds(&view, cx), [(0, 2), (4, 5)]);
}

#[gpui::test]
fn the_server_ranges_are_the_only_folds(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "main.rs");
    set(&view, cx, Some(vec![(4, 6)]));

    // A caret outside every range has nothing to fold: no indented block.
    view.update(cx, |view, _| view.model_mut().set_cursor(0));
    run(&view, cx, "editor.fold");
    assert_eq!(folds(&view, cx), []);

    // And no ranges at all folds nothing either.
    set(&view, cx, Some(vec![]));
    run(&view, cx, "editor.fold_all");
    assert_eq!(folds(&view, cx), []);
    assert_eq!(chevrons(&view, cx), Vec::<u32>::new());
}
