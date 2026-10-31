//! 'inccommand' in the view: a `:s` line's preview highlights its matches
//! and shows each row it changes as it would read, follows the line as it
//! is typed, reveals a match out of view, and a cancel puts the text, the
//! decorations and the view back; a submit leaves only the real edit.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_editor::decorations::{Decoration, DecorationKind};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, commands::RunCommand, init};

/// `r0` to `r499`, one a row, but `a2`, `a3` and `far a` on row 400.
fn text() -> String {
    (0..500)
        .map(|row| match row {
            2 | 3 => format!("a{row}\n"),
            400 => "far a\n".to_string(),
            row => format!("r{row}\n"),
        })
        .collect()
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text()), None),
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
    view.update(cx, |view, cx| view.preview_substitute(line, cx));
    draw(cx);
}

/// The highlighted ranges, the rows' text after their ends, the text, the
/// scroll and the caret.
fn shown(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Vec<std::ops::Range<usize>>, Vec<String>, String, f32, usize) {
    view.read_with(cx, |view, _| {
        let decorations = view.substitute_preview_decorations();
        let of = |decoration: &Decoration| match &decoration.kind {
            DecorationKind::AfterLine(after) => Some(after.to_string()),
            _ => None,
        };
        let mut highlights: Vec<_> = (decorations.iter())
            .filter(|decoration| decoration.kind == DecorationKind::Highlight)
            .map(|decoration| decoration.range.clone())
            .collect();
        highlights.sort_by_key(|range| range.start);
        (
            highlights,
            decorations.iter().filter_map(of).collect(),
            view.model().text().to_string(),
            view.scroll_top(),
            view.model().primary_selection().head(),
        )
    })
}

/// Keeps the previews from giving up on a loaded machine's clock.
fn unhurried() -> impl Drop {
    struct Unhurried;
    impl Drop for Unhurried {
        fn drop(&mut self) {
            fe_vim::set_search_preview_limit(None);
        }
    }
    fe_vim::set_search_preview_limit(Some(std::time::Duration::from_secs(60)));
    Unhurried
}

#[gpui::test]
fn preview_follows_typing_and_restores(cx: &mut TestAppContext) {
    let _unhurried = unhurried();
    let (view, cx) = editor(cx);
    let text = text();
    let at = |needle: &str| text.find(needle).unwrap();
    let (a2, a3, far) = (at("a2"), at("a3"), at("far a"));
    let before = shown(&view, cx);
    assert!(before.0.is_empty());
    assert_eq!(before.3, 0.);

    cx.simulate_keystrokes(":");
    draw(cx);
    // Typed a key at a time: nothing until the pattern's delimiter closes.
    for typed in ["%", "%s", "%s/", "%s/a"] {
        preview(&view, typed, cx);
        assert!(shown(&view, cx).0.is_empty(), "{typed}");
    }
    // Deleting the matches, then replacing them, a keystroke at a time.
    preview(&view, "%s/a/", cx);
    let (highlights, after, ..) = shown(&view, cx);
    assert_eq!(highlights, [a2..a2 + 1, a3..a3 + 1, far + 1..far + 2]);
    assert_eq!(after, ["→ 2", "→ 3", "→ fr a"]);
    preview(&view, "%s/a/b", cx);
    assert_eq!(shown(&view, cx).1, ["→ b2", "→ b3", "→ fbr a"]);
    preview(&view, "%s/a/b/", cx);
    assert_eq!(shown(&view, cx).1, ["→ b2", "→ b3", "→ fbr a"]);
    preview(&view, "%s/a/b/g", cx);
    assert_eq!(shown(&view, cx).1, ["→ b2", "→ b3", "→ fbr b"]);
    // In view from the caret: no scroll, the caret staying.
    let (_, _, typed_text, top, caret) = shown(&view, cx);
    assert_eq!((typed_text, top, caret), (text.clone(), 0., 0));
    // A match out of view is revealed; backing out to one in view, or to
    // nothing, puts the view back.
    preview(&view, "%s/far/near/", cx);
    let (highlights, after, _, top, caret) = shown(&view, cx);
    #[allow(clippy::single_range_in_vec_init)]
    let one = vec![far..far + 3];
    assert_eq!(highlights, one);
    assert_eq!(after, ["→ near a"]);
    assert!(top > 300., "{top}");
    assert_eq!(caret, 0);
    preview(&view, "%s/a/b/", cx);
    assert_eq!(shown(&view, cx).3, 0.);
    preview(&view, "%s/far/near/", cx);
    preview(&view, "%s/fa", cx);
    let (highlights, after, _, top, _) = shown(&view, cx);
    assert_eq!((highlights, after, top), (vec![], vec![], 0.));

    // Esc puts back the text, the decorations and the view.
    preview(&view, "%s/far/near/", cx);
    assert!(shown(&view, cx).3 > 300.);
    run("vim.command_line_cancel", "%s/far/near/", cx);
    assert_eq!(shown(&view, cx), before);

    // Enter runs the real :s, no preview left over.
    cx.simulate_keystrokes(":");
    draw(cx);
    preview(&view, "%s/a/b/", cx);
    assert_eq!(shown(&view, cx).0.len(), 3);
    run("vim.command_line_submit", "%s/a/b/", cx);
    let (highlights, after, ran, ..) = shown(&view, cx);
    assert_eq!((highlights, after), (vec![], vec![]));
    assert_eq!(
        ran,
        text.replace("a2", "b2")
            .replace("a3", "b3")
            .replace("far a", "fbr a")
    );

    // A line that isn't a :s previews nothing.
    cx.simulate_keystrokes(":");
    draw(cx);
    preview(&view, "%d", cx);
    assert!(shown(&view, cx).0.is_empty());
    run("vim.command_line_cancel", "", cx);

    // 'inccommand' off: nothing.
    cx.update(|_, cx| set_vim_inccommand(cx, false));
    cx.simulate_keystrokes(":");
    draw(cx);
    preview(&view, "%s/b/c/", cx);
    assert!(shown(&view, cx).0.is_empty());
    cx.update(|_, cx| set_vim_inccommand(cx, true));
    preview(&view, "%s/b/c/", cx);
    assert_eq!(shown(&view, cx).0.len(), 3);
    run("vim.command_line_cancel", "", cx);
    assert!(shown(&view, cx).0.is_empty());
}

#[gpui::test]
fn a_preview_comes_down_when_the_buffer_changes_or_the_setting_goes_off(cx: &mut TestAppContext) {
    let _unhurried = unhurried();
    let (view, cx) = editor(cx);
    let before = shown(&view, cx);

    // An edit from elsewhere (another view of the buffer, a server): the
    // decorations of the text before go, nothing else.
    cx.simulate_keystrokes(":");
    draw(cx);
    preview(&view, "%s/a/b/", cx);
    assert_eq!(shown(&view, cx).0.len(), 3);
    view.update(cx, |view, _| {
        view.model_mut()
            .buffer_mut()
            .edit([(0..1, "R")], fe_buffer::Origin::Lsp);
    });
    draw(cx);
    let (highlights, after, ..) = shown(&view, cx);
    assert_eq!((highlights, after), (vec![], vec![]));
    // The line's next edit previews afresh.
    preview(&view, "%s/a/b/", cx);
    assert_eq!(shown(&view, cx).0.len(), 3);
    run("vim.command_line_cancel", "", cx);

    // 'inccommand' going off takes down a preview at once, putting back the
    // view it revealed a match in.
    cx.simulate_keystrokes(":");
    draw(cx);
    preview(&view, "%s/far/near/", cx);
    assert!(shown(&view, cx).3 > 300.);
    cx.update(|_, cx| set_vim_inccommand(cx, false));
    draw(cx);
    let (highlights, after, _, top, _) = shown(&view, cx);
    assert_eq!((highlights, after, top), (vec![], vec![], before.3));
    cx.update(|_, cx| set_vim_inccommand(cx, true));
    run("vim.command_line_cancel", "", cx);
}

#[gpui::test]
fn a_match_of_line_breaks_alone_paints_at_its_row_end(cx: &mut TestAppContext) {
    let _unhurried = unhurried();
    let (view, cx) = editor(cx);
    cx.simulate_keystrokes(":");
    draw(cx);
    // `r0`'s line break, joining it to `r1`.
    preview(&view, r"1s/\n//", cx);
    let (highlights, after, ..) = shown(&view, cx);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0], 2..2);
    assert_eq!(after, ["→ r0r1"]);
    let painted = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        (layout.decorations.placed.iter())
            .filter(|placed| placed.kind == DecorationKind::Highlight)
            .map(|placed| placed.bounds)
            .collect::<Vec<_>>()
    });
    assert_eq!(painted.len(), 1, "{painted:?}");
    assert!(painted[0].size.width > gpui::px(0.), "{painted:?}");
    run("vim.command_line_cancel", "", cx);
}

#[gpui::test]
fn a_buffer_change_takes_down_the_whole_preview(cx: &mut TestAppContext) {
    let _unhurried = unhurried();
    let (view, cx) = editor(cx);
    let folds = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let folds = view.model().display().folds();
            folds
                .iter()
                .map(|fold| (fold.start, fold.end))
                .collect::<Vec<_>>()
        })
    };
    view.update(cx, |view, _| view.model.fold_rows(390..=420));
    draw(cx);
    let before = shown(&view, cx);
    assert_eq!(folds(&view, cx), [(390, 420)]);

    // The distant folded match is revealed, its fold opened.
    cx.simulate_keystrokes(":");
    draw(cx);
    preview(&view, "%s/far/near/", cx);
    assert!(shown(&view, cx).3 > 300.);
    assert_eq!(folds(&view, cx), []);
    // An edit from elsewhere takes all of it down: the decorations, the
    // view and the fold.
    view.update(cx, |view, _| {
        view.model_mut()
            .buffer_mut()
            .edit([(0..1, "R")], fe_buffer::Origin::Lsp);
    });
    draw(cx);
    let (highlights, after, _, top, caret) = shown(&view, cx);
    assert_eq!(
        (highlights, after, top, caret),
        (vec![], vec![], before.3, before.4)
    );
    assert_eq!(folds(&view, cx), [(390, 420)]);
    // 'inccommand' going off then leaves it so.
    cx.update(|_, cx| set_vim_inccommand(cx, false));
    draw(cx);
    assert_eq!(shown(&view, cx).3, before.3);
    assert_eq!(folds(&view, cx), [(390, 420)]);
    cx.update(|_, cx| set_vim_inccommand(cx, true));
    run("vim.command_line_cancel", "", cx);
    assert_eq!(folds(&view, cx), [(390, 420)]);
}
