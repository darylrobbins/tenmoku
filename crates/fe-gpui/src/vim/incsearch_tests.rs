//! 'incsearch' in the view: a search line's preview paints its match as
//! the current one and its pattern's matches in place of any other, scrolls
//! the match into view from where the view was before the line opened
//! (past blocks, along a long row, into a fold), and a cancel puts the view,
//! the folds and the highlight back.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_vim::search::{Pattern, regex_calls, set_call_limit};
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext};

use super::*;
use crate::{
    EditorTheme, EditorView,
    commands::RunCommand,
    init,
    view::{HostBlock, HostBlockKind},
};

/// `r0` to `r499`, one a row, `far` on row 400.
fn text() -> String {
    (0..500)
        .map(|row| match row {
            400 => "far\n".to_string(),
            row => format!("r{row}\n"),
        })
        .collect()
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor_with(cx, text())
}

fn editor_with(
    cx: &mut TestAppContext,
    text: String,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn run(command: &str, text: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command)
        .unwrap()
        .args(serde_json::json!({ "text": text }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

fn preview(view: &Entity<EditorView>, line: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.preview_search(line, cx));
    draw(cx);
}

/// The previewed match, whether the frame painted it inside the text area,
/// the scroll and the pattern the view's matches paint by.
fn shown(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Option<std::ops::Range<usize>>, bool, f32, Option<String>) {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let area = layout.bounds(layout.frame.text_bounds());
        let inside = |rect: &Bounds<Pixels>| {
            rect.left() >= area.left()
                && rect.right() <= area.right()
                && rect.top() >= area.top()
                && rect.bottom() <= area.bottom()
        };
        let painted = &view.painted_current_match;
        (
            view.search_preview(),
            !painted.is_empty() && painted.iter().all(inside),
            view.scroll_top(),
            view.match_pattern()
                .map(|pattern| pattern.regex_str().to_string()),
        )
    })
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

#[gpui::test]
fn a_preview_reveals_its_match_and_a_cancel_puts_the_view_back(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let far = text().find("far").unwrap();
    // An earlier search's highlight.
    cx.simulate_keystrokes("/");
    run("vim.command_line_submit", "r1", cx);
    let earlier = shown(&view, cx).3;
    cx.simulate_keystrokes("g g");
    draw(cx);
    assert_eq!(shown(&view, cx).2, 0.);

    cx.simulate_keystrokes("/");
    draw(cx);
    // Nothing typed: the earlier highlight, nothing current.
    preview(&view, "", cx);
    assert_eq!(shown(&view, cx), (None, false, 0., earlier.clone()));
    preview(&view, "fa", cx);
    let (found, painted, top, pattern) = shown(&view, cx);
    assert_eq!(found, Some(far..far + 2));
    assert!(painted);
    assert!(top > 300., "{top}");
    assert_ne!(pattern, earlier);
    // The caret stays where the line opened.
    assert_eq!(caret(&view, cx), 0);
    // A match in the view the line opened on shows it there again.
    preview(&view, "r2", cx);
    assert_eq!(shown(&view, cx).2, 0.);
    preview(&view, "far", cx);
    assert!(shown(&view, cx).2 > 300.);
    // Nothing found: the view and the earlier highlight are back.
    preview(&view, "farther", cx);
    assert_eq!(shown(&view, cx), (None, false, 0., earlier.clone()));
    preview(&view, "far", cx);
    assert!(shown(&view, cx).2 > 300.);

    // Cancelling puts them back.
    run("vim.command_line_cancel", "far", cx);
    assert_eq!(shown(&view, cx), (None, false, 0., earlier.clone()));
    assert_eq!(caret(&view, cx), 0);

    // Submitting lands on the match, in view.
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    let top = shown(&view, cx).2;
    run("vim.command_line_submit", "far", cx);
    assert_eq!(caret(&view, cx), far);
    let (found, painted, landed, pattern) = shown(&view, cx);
    assert_eq!((found, painted, landed), (None, false, top));
    assert_ne!(pattern, earlier);
}

#[gpui::test]
fn a_cancel_puts_back_a_view_scrolled_away_from_the_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| view.set_scroll_top(200., cx));
    draw(cx);
    // Opening the line would scroll to the caret: a cancel puts the view
    // back, previewed or not.
    for typed in [None, Some("r210"), Some("far"), Some("nowhere")] {
        cx.simulate_keystrokes("/");
        draw(cx);
        if let Some(typed) = typed {
            preview(&view, typed, cx);
            let top = shown(&view, cx).2;
            match typed {
                // In the view before the line opened: shown there.
                "r210" => assert_eq!(top, 200.),
                "far" => assert!(top > 300., "{top}"),
                _ => {}
            }
        }
        run("vim.command_line_cancel", "", cx);
        assert_eq!(shown(&view, cx).2, 200., "{typed:?}");
        assert_eq!(caret(&view, cx), 0);
    }
}

#[gpui::test]
fn a_preview_reveals_a_match_past_the_right_edge(cx: &mut TestAppContext) {
    let wide = format!("{}far\nshort\n", "x".repeat(400));
    let (view, cx) = editor_with(cx, wide);
    let scroll_x = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| f32::from(view.scroll.x))
    };
    assert_eq!(scroll_x(&view, cx), 0.);
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    let (found, painted, _, _) = shown(&view, cx);
    assert_eq!(found, Some(400..403));
    assert!(painted);
    assert!(scroll_x(&view, cx) > 0.);
    // Back at the left edge for a match there, and on a cancel.
    preview(&view, "short", cx);
    assert!(shown(&view, cx).1);
    assert_eq!(scroll_x(&view, cx), 0.);
    preview(&view, "far", cx);
    assert!(scroll_x(&view, cx) > 0.);
    run("vim.command_line_cancel", "", cx);
    assert_eq!(scroll_x(&view, cx), 0.);
}

#[gpui::test]
fn a_preview_opens_the_fold_hiding_its_match(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let folded = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.model().display().hidden_rows(400).is_some()
        })
    };
    view.update(cx, |view, _| view.model.fold_rows(390..=420));
    draw(cx);
    assert!(folded(&view, cx));
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    assert!(!folded(&view, cx));
    assert!(shown(&view, cx).1);
    // Another match, or none, folds it again; a cancel too.
    preview(&view, "r1", cx);
    assert!(folded(&view, cx));
    preview(&view, "far", cx);
    assert!(!folded(&view, cx));
    run("vim.command_line_cancel", "", cx);
    assert!(folded(&view, cx));
    // A submit leaves it open, the caret on the match.
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    run("vim.command_line_submit", "far", cx);
    assert!(!folded(&view, cx));
    assert_eq!(caret(&view, cx), text().find("far").unwrap());
}

#[gpui::test]
fn a_preview_reveals_a_match_below_a_tall_block(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let rows = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.visible_rows()
    });
    // A block as tall as the view above row 5: row 10 is within a view's
    // count of display rows from the top, but off the screen.
    let block = HostBlock {
        above: 5,
        rows,
        kind: HostBlockKind::Filler,
        id: 1,
    };
    view.update(cx, |view, cx| view.set_blocks(vec![block], cx));
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.shows_buffer_row(10)),
        Some(false)
    );
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "r10", cx);
    assert!(shown(&view, cx).1);
    assert_eq!(
        view.read_with(cx, |view, _| view.shows_buffer_row(10)),
        Some(true)
    );
    run("vim.command_line_cancel", "", cx);
    assert_eq!(shown(&view, cx).2, 0.);
}

/// Gives every search deadline a budget of regex calls for its scope.
struct CallLimited;

impl CallLimited {
    fn of(calls: u64) -> Self {
        set_call_limit(Some(calls));
        Self
    }
}

impl Drop for CallLimited {
    fn drop(&mut self) {
        set_call_limit(None);
    }
}

#[gpui::test]
fn a_previews_matches_paint_within_its_time_limit(cx: &mut TestAppContext) {
    // Each `a` scans to the end of its row for a `z`: the match the line
    // lands on is found in one regex call, the rest of the row's take one
    // call each. The deadlines run out after a counted number of calls, not
    // a measured time, so the test doesn't depend on the machine's speed.
    let text: String = (0..40).map(|_| "a".repeat(5_000) + "\n").collect();
    let (view, cx) = editor_with(cx, text);
    cx.simulate_keystrokes("/");
    draw(cx);
    let limit = CallLimited::of(500);
    let before = regex_calls();
    preview(&view, r"a.*z\|a", cx);
    let calls = regex_calls() - before;
    drop(limit);
    assert_eq!(shown(&view, cx).0, Some(1..2));
    assert!(view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().highlight_ran_out()
    }));
    // The highlight was cut short, well before the end of the first row.
    assert!(calls < 5_000, "{calls}");
}

#[gpui::test]
fn a_preview_paints_over_the_find_pattern(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| {
        view.set_find(Some(Pattern::parse("r1").unwrap()), cx)
    });
    draw(cx);
    let find = shown(&view, cx).3;
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    let previewed = shown(&view, cx).3;
    assert_ne!(previewed, find);
    assert_eq!(
        previewed.as_deref(),
        Some(Pattern::parse("far").unwrap().regex_str())
    );
    run("vim.command_line_cancel", "", cx);
    assert_eq!(shown(&view, cx).3, find);
}

#[gpui::test]
fn turning_vim_off_ends_the_preview(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    assert!(shown(&view, cx).2 > 300.);
    cx.update(|_, cx| set_vim_enabled(cx, false));
    draw(cx);
    assert_eq!(shown(&view, cx), (None, false, 0., None));
}

#[gpui::test]
fn incsearch_off_previews_nothing(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, _| {
        view.vim.as_mut().unwrap().set_incsearch(false)
    });
    cx.simulate_keystrokes("/");
    draw(cx);
    preview(&view, "far", cx);
    assert_eq!(shown(&view, cx), (None, false, 0., None));
}
