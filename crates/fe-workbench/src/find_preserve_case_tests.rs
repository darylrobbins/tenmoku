//! The find bar's Preserve Case: `find.toggle_preserve_case` and the `AB`
//! toggle, which Replace and Replace All follow, with the setup of
//! [`crate::tests::find_bar`].

use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Pane,
    tests::find_bar::{draw, editor, focus, opened, pane, run, select, text_label},
};

fn text(editor: &Entity<fe_gpui::EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn preserving(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| pane.find_bar().unwrap().preserves_case())
}

/// Focuses the query input, or with `replace` the replace input, clears
/// it and types `text` there.
fn type_in(pane: &Entity<Pane>, replace: bool, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let bar = pane.read(cx).find_bar().unwrap();
        let input = match replace {
            true => bar.replace_input(),
            false => bar.input(),
        };
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
    run("text_input.clear", cx);
    cx.simulate_input(text);
    draw(cx);
}

/// Takes the last replace back, from the editor, and goes back to the
/// replace input.
fn undo(pane: &Entity<Pane>, editor: &Entity<fe_gpui::EditorView>, cx: &mut VisualTestContext) {
    focus(editor, cx);
    run("editor.undo", cx);
    cx.update(|window, cx| {
        let input = pane.read(cx).find_bar().unwrap().replace_input().clone();
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
}

#[gpui::test]
fn replace_all(cx: &mut TestAppContext) {
    let original = "foo Foo FOO\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 3"));
    type_in(&pane, true, "bar", cx);

    // Off by default: the replacement as written.
    assert!(!preserving(&pane, cx));
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "bar bar bar\n");
    undo(&pane, &editor, cx);
    assert_eq!(text(&editor, cx), original);

    // The toggle, beside the replace buttons, turns it on, focus staying
    // in the replace input; Replace All is one undo step still.
    let toggle = cx.debug_bounds("find-preserve-case").expect("the toggle");
    cx.simulate_click(toggle.center(), Modifiers::none());
    draw(cx);
    assert!(preserving(&pane, cx));
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "bar Bar BAR\n");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 3"));
    undo(&pane, &editor, cx);
    assert_eq!(text(&editor, cx), original);

    // Replace, one match at a time, follows it too.
    select(&editor, 0..3, cx);
    for want in ["bar Foo FOO\n", "bar Bar FOO\n", "bar Bar BAR\n"] {
        run("find.replace_next", cx);
        assert_eq!(text(&editor, cx), want);
    }
    for _ in 0..3 {
        undo(&pane, &editor, cx);
    }
    assert_eq!(text(&editor, cx), original);

    // Capture groups expand first, and the expansion takes the case: `Foo`
    // makes `ooF`, then `OoF`.
    type_in(&pane, false, "(f)(o+)", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, true, "$2$1", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "oof OoF OOF\n");
    undo(&pane, &editor, cx);
    select(&editor, 0..3, cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "oof Foo FOO\n");
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "oof OoF FOO\n");
    undo(&pane, &editor, cx);
    undo(&pane, &editor, cx);

    // The key toggles it off again: as written.
    run("find.toggle_preserve_case", cx);
    assert!(!preserving(&pane, cx));
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "oof ooF OOF\n");
}
