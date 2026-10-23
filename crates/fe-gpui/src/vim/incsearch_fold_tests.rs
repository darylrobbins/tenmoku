//! The folds a search preview opened follow the buffer's edits made before
//! the preview is cancelled or replaced, as the display's own folds do.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// `r0` to `r499`, one a row, `far` on row 400.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let text: String = (0..500)
        .map(|row| match row {
            400 => "far\n".to_string(),
            row => format!("r{row}\n"),
        })
        .collect();
    // The previews below must find `far` however loaded the machine is, so
    // neither wall-clock limit on matching may cut them off.
    fe_vim::set_search_preview_limit(Some(std::time::Duration::from_secs(600)));
    fe_vim::search::set_time_limit(std::time::Duration::from_secs(600));
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
    cx.run_until_parked();
    (view, cx)
}

fn folds(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, u32)> {
    view.read_with(cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    })
}

/// Previews `far` over the fold `390..=420`, which it opens, then runs `edit`
/// on the model.
fn preview_then(
    view: &Entity<EditorView>,
    edit: impl FnOnce(&mut EditorModel),
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, _| view.model.fold_rows(390..=420));
    cx.simulate_keystrokes("/");
    view.update(cx, |view, cx| view.preview_search("far", cx));
    assert_eq!(folds(view, cx), []);
    view.update(cx, |view, cx| {
        edit(view.model_mut());
        cx.notify();
    });
}

/// A row inserted above the fold before the cancel moves the fold it puts
/// back, instead of folding the rows that took its place.
#[gpui::test]
fn a_cancel_folds_where_the_edits_left_the_fold(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    preview_then(
        &view,
        |model| {
            model.set_cursor(0);
            model.paste("new\n");
        },
        cx,
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(folds(&view, cx), [(391, 421)]);
}

/// A replacing preview does the same.
#[gpui::test]
fn a_new_preview_folds_where_the_edits_left_the_fold(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    preview_then(
        &view,
        |model| {
            model.set_cursor(0);
            model.paste("new\n");
        },
        cx,
    );
    view.update(cx, |view, cx| view.preview_search("r1", cx));
    assert_eq!(folds(&view, cx), [(391, 421)]);
}

/// An edit inside the fold drops it, as it would have dropped the fold
/// were it still folded.
#[gpui::test]
fn an_edit_inside_the_fold_drops_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    preview_then(
        &view,
        |model| {
            let at = model.text().point_to_offset(fe_buffer::Point::new(405, 0));
            model.set_cursor(at);
            model.paste("x");
        },
        cx,
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(folds(&view, cx), []);
}

/// Edits in separate batches either side of the fold leave it: it moves
/// with the one above, and the one below doesn't touch it.
#[gpui::test]
fn edits_on_both_sides_keep_the_fold(cx: &mut TestAppContext) {
    for replace in [false, true] {
        let (view, cx) = editor(cx);
        preview_then(
            &view,
            |model| {
                model.set_cursor(0);
                model.paste("new\n");
            },
            cx,
        );
        view.update(cx, |view, cx| {
            let model = view.model_mut();
            let at = model.text().point_to_offset(fe_buffer::Point::new(450, 0));
            model.set_cursor(at);
            model.paste("below\n");
            cx.notify();
        });
        match replace {
            true => view.update(cx, |view, cx| view.preview_search("r1", cx)),
            false => cx.simulate_keystrokes("escape"),
        }
        assert_eq!(folds(&view, cx), [(391, 421)], "replace {replace}");
    }
}

/// An edit in the fold's header that adds no line break, then another
/// below, keeps the fold where it was.
#[gpui::test]
fn a_header_edit_and_a_later_edit_keep_the_fold(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    preview_then(
        &view,
        |model| {
            let at = model.text().point_to_offset(fe_buffer::Point::new(390, 0));
            model.set_cursor(at);
            model.paste("x");
        },
        cx,
    );
    view.update(cx, |view, cx| {
        let model = view.model_mut();
        let at = model.text().point_to_offset(fe_buffer::Point::new(450, 0));
        model.set_cursor(at);
        model.paste("below\n");
        cx.notify();
    });
    cx.simulate_keystrokes("escape");
    assert_eq!(folds(&view, cx), [(390, 420)]);
}
