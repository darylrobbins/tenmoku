//! Carets added in vim's normal mode, which its commands act at together,
//! and `escape` going back to one.

use super::*;

/// Each caret's offset, or `None` if there's a selection.
fn carets(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Vec<usize>> {
    view.read_with(cx, |view, _| {
        let selections = view.model().selections();
        (selections.iter())
            .all(|selection| selection.range.is_empty())
            .then(|| {
                selections
                    .iter()
                    .map(|selection| selection.head())
                    .collect()
            })
    })
}

#[gpui::test]
fn gb_adds_carets_that_vim_edits_together(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "foo bar foo foobar foo", 1);
    cx.simulate_keystrokes("g b");
    assert_eq!(carets(&view, cx), Some(vec![1, 9]));
    cx.simulate_keystrokes("g b");
    assert_eq!(carets(&view, cx), Some(vec![1, 9, 20]));
    assert_eq!(mode(&view, cx), Some("normal"));

    cx.simulate_keystrokes("c i w");
    assert_eq!(mode(&view, cx), Some("insert"));
    cx.simulate_input("x");
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&view, cx), "x bar x foobar x");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(carets(&view, cx).map(|carets| carets.len()), Some(3));

    // Escape again: only the primary caret.
    cx.simulate_keystrokes("escape");
    assert_eq!(carets(&view, cx), Some(vec![15]));
    assert_eq!(mode(&view, cx), Some("normal"));

    // One undo step takes the three changes back.
    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), "foo bar foo foobar foo");
}

#[gpui::test]
fn macos_keys_add_carets_in_normal_mode(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "foo bar\nfoo baz foo", 0);
    cx.simulate_keystrokes("cmd-d");
    assert_eq!(carets(&view, cx), Some(vec![0, 8]));
    cx.simulate_keystrokes("escape");
    assert_eq!(carets(&view, cx), Some(vec![8]));
    cx.simulate_keystrokes("cmd-shift-l");
    assert_eq!(carets(&view, cx), Some(vec![0, 8, 16]));
    cx.simulate_keystrokes("escape g g 0 cmd-alt-down");
    assert_eq!(carets(&view, cx), Some(vec![0, 8]));
    // `x` deletes at both.
    cx.simulate_keystrokes("x");
    assert_eq!(text(&view, cx), "oo bar\noo baz foo");
}
