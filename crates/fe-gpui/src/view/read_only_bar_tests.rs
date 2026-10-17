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
