//! A read-only editor refuses an ex line if any command `|` separates
//! edits: `:y | d` is E21 and changes nothing, `:y | noh` runs.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn read_only_refuses_a_bar_line_that_edits(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "foo bar\nbaz foo\n", 0, true);
    set_read_only(&view, true, cx);
    let before = state(&view, cx);
    for line in ["y | d", "noh | s/foo/X/", "g/foo/y | d"] {
        submit(":", line, cx);
        assert_eq!(state(&view, cx), before, ":{line}");
        assert_eq!(mode(&view, cx), Some("normal"), ":{line}");
        assert_eq!(message(&view, cx).as_deref(), Some(E21), ":{line}");
    }
    submit(":", "y | noh", cx);
    assert_eq!(state(&view, cx), before);
    assert_eq!(message(&view, cx), None);
    assert_eq!(register('"', cx).as_deref(), Some("foo bar\n"));
}

/// Folding changes no text, so a read-only view makes folds: `zfj`, `3zF`,
/// visual `zf`, `zf/` and `:normal zfj`, without E21.
#[gpui::test]
fn read_only_makes_folds(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "foo\nbar\nbaz\nqux\n", 0, true);
    set_read_only(&view, true, cx);
    let folds =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model.display().folds().len());
    for keys in ["z f j", "3 z shift-f", "v j z f"] {
        cx.simulate_keystrokes("z shift-e g g");
        cx.simulate_keystrokes(keys);
        assert_eq!(folds(cx), 1, "{keys}");
        assert_eq!(message(&view, cx), None, "{keys}");
    }
    cx.simulate_keystrokes("z shift-e g g");
    submit("z f /", "baz", cx);
    assert_eq!(folds(cx), 1, "zf/");
    assert_eq!(message(&view, cx), None, "zf/");
    cx.simulate_keystrokes("z shift-e g g");
    submit(":", "normal zfj", cx);
    assert_eq!(folds(cx), 1, ":normal zfj");
    assert_eq!(message(&view, cx), None, ":normal zfj");
}
