//! Expand and shrink selection follow the parsed syntax tree.

use fe_command::Invocation;
use gpui::Focusable;

use super::*;
use crate::commands::RunCommand;

fn run(id: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(id).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> std::ops::Range<usize> {
    view.read_with(cx, |view, _| view.model().primary_selection().range)
}

#[gpui::test]
fn expand_follows_the_syntax_tree(cx: &mut TestAppContext) {
    let text = "fn main() { foo(1, 2); }\n";
    let (view, cx) = editor(text, Some("main.rs"), cx);
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    let caret = text.find("2").unwrap();
    view.update(cx, |view, _| view.model_mut().set_cursor(caret));
    let mut expected = caret..caret;
    for _ in 0..3 {
        expected = view.read_with(cx, |view, _| {
            fe_syntax::larger_syntax_node(view.syntax().unwrap(), expected.clone()).unwrap()
        });
        run("editor.expand_selection", cx);
        assert_eq!(selection(&view, cx), expected);
    }
    for _ in 0..3 {
        run("editor.shrink_selection", cx);
    }
    assert_eq!(selection(&view, cx), caret..caret);
}
