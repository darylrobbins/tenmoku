//! Diagnostics in a headless window: squiggles in each severity's colour,
//! a gutter mark per row with the most severe, and each message in the
//! hover popover once the pointer has rested on it.

use std::{ops::Range, time::Duration};

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, Severity},
};
use gpui::{Entity, Hsla, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px};

use crate::{hover::HOVER_DELAY, *};

const TEXT: &str = "fn main() {\n    let x = y + z;\n}\n";

/// The identity the host applies them from: server 1's (1, 1).
const ID: (u64, u64) = (1, 1);

fn word(word: &str) -> Range<usize> {
    let start = TEXT.find(word).unwrap();
    start..start + word.len()
}

/// A plain-text editor on [`TEXT`], drawn, in an active window.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view, cx)
}

/// Mid-row, just right of the caret position at `offset`, in the last
/// frame.
fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    })
}

/// The window x of the caret at `offset`.
fn x(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Pixels {
    view.update(cx, |view, _| {
        view.layout
            .as_mut()
            .unwrap()
            .caret_bounds(offset)
            .unwrap()
            .left()
    })
}

/// The last frame's squiggles, gutter marks and popover text.
type Painted = (
    Vec<(gpui::Bounds<Pixels>, gpui::UnderlineStyle)>,
    Vec<(u32, Hsla)>,
    Option<String>,
);

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Painted {
    view.read_with(cx, |view, _| {
        (
            view.painted_squiggles.clone(),
            view.painted_marks.clone(),
            view.painted_popover.clone(),
        )
    })
}

#[gpui::test]
fn squiggles_gutter_and_hover(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let unmarked = view.read_with(cx, |view, _| {
        (view.painted_carets.clone(), view.painted_selections.clone())
    });
    // None yet: nothing painted.
    assert_eq!(painted(&view, cx), (Vec::new(), Vec::new(), None));

    let items = vec![
        (
            word("main"),
            Diagnostic::new(Severity::Information, "main info"),
        ),
        (word("x"), Diagnostic::new(Severity::Error, "x error")),
        (word("y"), Diagnostic::new(Severity::Warning, "y warning")),
        (word("z"), Diagnostic::new(Severity::Hint, "z hint")),
    ];
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, ID, version, items.clone(), Some(ID))
        );
        cx.notify();
    });
    cx.run_until_parked();

    // A wavy underline under each range, in its severity's colour, the
    // most severe painted last.
    let (squiggles, marks, popover) = painted(&view, cx);
    let mut expected: Vec<_> = items
        .iter()
        .map(|(range, d)| (range.clone(), d.severity))
        .collect();
    expected.sort_by_key(|(_, severity)| *severity);
    assert_eq!(squiggles.len(), expected.len());
    for ((bounds, style), (range, severity)) in squiggles.iter().zip(&expected) {
        assert!(style.wavy);
        assert_eq!(style.color, Some(theme.diagnostic_color(*severity)));
        assert_eq!(bounds.left(), x(&view, range.start, cx));
        assert_eq!(bounds.right(), x(&view, range.end, cx));
    }
    // One mark per row, in the most severe's colour.
    assert_eq!(
        marks,
        [
            (0, theme.diagnostic_color(Severity::Information)),
            (1, theme.diagnostic_color(Severity::Error)),
        ]
    );
    assert_eq!(popover, None);

    // Resting on `y` shows its message once the delay is up.
    let y = at(&view, word("y").start, cx);
    cx.simulate_mouse_move(y, None, Modifiers::none());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(HOVER_DELAY - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(painted(&view, cx).2, None);
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(painted(&view, cx).2.as_deref(), Some("y warning"));

    // Off it, it goes; onto another, that one's shows after the delay.
    let blank = at(&view, word("let").start - 1, cx);
    cx.simulate_mouse_move(blank, None, Modifiers::none());
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(painted(&view, cx).2, None);
    let main = at(&view, word("main").start + 1, cx);
    cx.simulate_mouse_move(main, None, Modifiers::none());
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(painted(&view, cx).2.as_deref(), Some("main info"));

    // Cleared, the paint is as before.
    cx.simulate_mouse_move(blank, None, Modifiers::none());
    view.update(cx, |view, cx| {
        assert!(view.model_mut().clear_diagnostics(1u32, ID));
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), (Vec::new(), Vec::new(), None));
    let after = view.read_with(cx, |view, _| {
        (view.painted_carets.clone(), view.painted_selections.clone())
    });
    assert_eq!(after, unmarked);
}

/// A plain-text editor on `text`, drawn, in an active window.
fn editor_on<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view, cx)
}

/// Applies `items` from [`ID`] at the current version, and draws.
fn apply(
    view: &Entity<EditorView>,
    items: Vec<(Range<usize>, Diagnostic)>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, ID, version, items, Some(ID))
        );
        cx.notify();
    });
    cx.run_until_parked();
}

/// Rests the pointer at `position` for the hover delay: the popover's
/// text then.
fn rest(
    view: &Entity<EditorView>,
    position: Point<Pixels>,
    cx: &mut VisualTestContext,
) -> Option<String> {
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.run_until_parked();
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    // Past the grace a popover the pointer left has to hide, and the
    // delay from there.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    painted(view, cx).2
}

fn row_start(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model.text().row_start(row))
}

/// The buffer rows the last frame showed.
fn shown_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let rows = &view.layout.as_ref().unwrap().frame.rows;
        rows.iter().map(|row| row.buffer_row).collect()
    })
}

#[gpui::test]
fn gutter_marks_cover_every_row(cx: &mut TestAppContext) {
    let text: String = (0..100).map(|row| format!("r{row} word\n")).collect();
    let (view, cx) = editor_on(&text, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let at = |row: u32, cx: &mut VisualTestContext| row_start(&view, row, cx);
    let (r2, r3, r4, r5, r7) = (at(2, cx), at(3, cx), at(4, cx), at(5, cx), at(7, cx));
    let (r10, r60) = (at(10, cx), at(60, cx));
    apply(
        &view,
        vec![
            // Ends at row 4's start, so not on it.
            (
                r2 + 1..r4,
                Diagnostic::new(Severity::Warning, "two to three"),
            ),
            // Over the warning on row 3.
            (r3..r3 + 2, Diagnostic::new(Severity::Error, "three")),
            (
                r5 + 1..r7 + 1,
                Diagnostic::new(Severity::Hint, "five to seven"),
            ),
            (
                r10 + 1..r60 + 1,
                Diagnostic::new(Severity::Information, "ten to sixty"),
            ),
        ],
        cx,
    );
    let color = |severity| theme.diagnostic_color(severity);
    let expected = |rows: &[u32]| -> Vec<(u32, Hsla)> {
        (rows.iter().copied())
            .filter_map(|row| match row {
                2 => Some((row, color(Severity::Warning))),
                3 => Some((row, color(Severity::Error))),
                5..=7 => Some((row, color(Severity::Hint))),
                10..=60 => Some((row, color(Severity::Information))),
                _ => None,
            })
            .collect()
    };
    let rows = shown_rows(&view, cx);
    assert!(rows.contains(&12) && !rows.contains(&60), "{rows:?}");
    assert_eq!(painted(&view, cx).1, expected(&rows));

    // Scrolled past where the last one starts, its rows keep their marks.
    view.update(cx, |view, cx| {
        let line_height = view.layout.as_ref().unwrap().frame.line_height;
        view.scroll.y = px(30. * line_height);
        cx.notify();
    });
    cx.run_until_parked();
    let rows = shown_rows(&view, cx);
    assert_eq!(rows.first(), Some(&30));
    let marks = painted(&view, cx).1;
    assert_eq!(marks, expected(&rows));
    assert_eq!(marks[0], (30, color(Severity::Information)));

    // A popover shown, then the caret revealed far below: it goes with
    // the frame that scrolls.
    let row_31 = at(31, cx);
    let over = super::diagnostics_tests::at(&view, row_31 + 1, cx);
    assert_eq!(rest(&view, over, cx).as_deref(), Some("ten to sixty"));
    let r90 = at(90, cx);
    view.update(cx, |view, cx| view.select_and_reveal(r90..r90, cx));
    cx.run_until_parked();
    assert!(shown_rows(&view, cx).contains(&90));
    assert_eq!(painted(&view, cx).2, None);
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(painted(&view, cx).2, None);
}

#[gpui::test]
fn row_end_hover_and_changed_diagnostics(cx: &mut TestAppContext) {
    // A row with text, an empty row, and an empty last row.
    let text = "one\n\nlet x\n";
    let (view, cx) = editor_on(text, cx);
    let x = text.find('x').unwrap();
    apply(
        &view,
        vec![
            (3..3, Diagnostic::new(Severity::Error, "row end")),
            (4..4, Diagnostic::new(Severity::Error, "empty row")),
            (x..x + 1, Diagnostic::new(Severity::Warning, "x")),
            (
                text.len()..text.len(),
                Diagnostic::new(Severity::Error, "eof"),
            ),
        ],
        cx,
    );
    assert_eq!(painted(&view, cx).0.len(), 4);

    // Each empty one shows its message from its space-wide squiggle past
    // the row's end.
    for (offset, message) in [(3, "row end"), (4, "empty row"), (text.len(), "eof")] {
        let over = at(&view, offset, cx);
        assert_eq!(rest(&view, over, cx).as_deref(), Some(message));
    }
    // Further right than that, nothing does.
    let past = at(&view, 3, cx) + point(px(40.), px(0.));
    assert_eq!(rest(&view, past, cx), None);

    // With the pointer still on `x`, a publication keeping its diagnostic
    // and adding one over it: the popover no longer stands as it was.
    let over_x = at(&view, x, cx);
    assert_eq!(rest(&view, over_x, cx).as_deref(), Some("x"));
    let let_x = text.find("let").unwrap()..x + 1;
    apply(
        &view,
        vec![
            (x..x + 1, Diagnostic::new(Severity::Warning, "x")),
            (let_x, Diagnostic::new(Severity::Error, "let x")),
        ],
        cx,
    );
    assert_ne!(painted(&view, cx).2.as_deref(), Some("x"));
    // Moved within it, the pointer finds both.
    let again = at(&view, x, cx) + point(px(1.), px(0.));
    assert_eq!(rest(&view, again, cx).as_deref(), Some("let x\nx"));
}

#[gpui::test]
fn moves_within_a_range_follow_the_pointer(cx: &mut TestAppContext) {
    let text = "let alpha = 1;\n";
    let (view, cx) = editor_on(text, cx);
    let alpha = text.find("alpha").unwrap();
    let a = || (alpha..alpha + 5, Diagnostic::new(Severity::Warning, "a"));
    // Over only the grapheme the pointer moves on to.
    let b = || (alpha + 3..alpha + 4, Diagnostic::new(Severity::Error, "b"));
    let move_to = |offset: usize, cx: &mut VisualTestContext| {
        let position = at(&view, offset, cx);
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.run_until_parked();
    };
    apply(&view, vec![a()], cx);

    // Shown, then moved within it: a publication adding one under the
    // pointer's new place doesn't leave the popover saying only "a".
    let start = at(&view, alpha, cx);
    assert_eq!(rest(&view, start, cx).as_deref(), Some("a"));
    move_to(alpha + 3, cx);
    assert_eq!(painted(&view, cx).2.as_deref(), Some("a"));
    apply(&view, vec![a(), b()], cx);
    assert_eq!(painted(&view, cx).2, None);

    // The same while waiting out the delay.
    apply(&view, vec![a()], cx);
    move_to(alpha, cx);
    cx.executor().advance_clock(HOVER_DELAY / 2);
    move_to(alpha + 3, cx);
    apply(&view, vec![a(), b()], cx);
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(painted(&view, cx).2, None);
}

#[path = "diagnostic_related_tests.rs"]
mod diagnostic_related_tests;
