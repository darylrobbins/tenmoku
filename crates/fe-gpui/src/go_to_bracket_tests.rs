use std::{cell::RefCell, rc::Rc};

use fe_command::Invocation;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{
    EditorView,
    commands::EditorViewEvent,
    tests::{editor, state},
};

fn run(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let invocation = Invocation::of("editor.go_to_bracket").unwrap();
        view.run_command(invocation, None, cx);
    });
}

fn rust<'a>(
    cx: &'a mut TestAppContext,
    marked: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (view, cx) = editor(cx, marked);
    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    (view, cx)
}

#[gpui::test]
fn partner(cx: &mut TestAppContext) {
    let (view, cx) = rust(cx, "const A: u8 = g(|a, b);");
    run(&view, cx);
    assert_eq!(state(&view, cx), "const A: u8 = g(a, b|);");
    run(&view, cx);
    assert_eq!(state(&view, cx), "const A: u8 = g(|a, b);");

    let (view, cx) = rust(cx, "const A: u8 = g|(a) + h|(b);");
    run(&view, cx);
    assert_eq!(state(&view, cx), "const A: u8 = g(a)| + h(b)|;");
    run(&view, cx);
    assert_eq!(state(&view, cx), "const A: u8 = g|(a) + h|(b);");
}

#[gpui::test]
fn enclosing(cx: &mut TestAppContext) {
    let (view, cx) = rust(cx, "const A: u8 = foo(b|ar, baz);");
    run(&view, cx);
    assert_eq!(state(&view, cx), "const A: u8 = foo(bar, baz|);");

    let (view, cx) = rust(cx, "const |A: u8 = 1;");
    run(&view, cx);
    assert_eq!(state(&view, cx), "const |A: u8 = 1;");

    let (view, cx) = rust(cx, "|const A: u8 = 1;");
    run(&view, cx);
    assert_eq!(state(&view, cx), "|const A: u8 = 1;");

    // A tree that lags the text moves nothing.
    let (view, cx) = rust(cx, "const A: u8 = foo(b|ar);");
    view.update(cx, |view, cx| {
        view.model.insert("x");
        let invocation = Invocation::of("editor.go_to_bracket").unwrap();
        view.run_command(invocation, None, cx);
    });
    assert_eq!(state(&view, cx), "const A: u8 = foo(bx|ar);");
}

#[gpui::test]
fn jump_list(cx: &mut TestAppContext) {
    let (view, cx) = rust(cx, "const A: u8 = g(|a);");
    let heard = Rc::new(RefCell::new(Vec::new()));
    let sink = heard.clone();
    let from = view.read_with(cx, |view, _| view.model.primary_selection().head());
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &EditorViewEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach()
    });
    run(&view, cx);
    assert_eq!(*heard.borrow(), [EditorViewEvent::Jumped { from }]);
}

#[gpui::test]
fn a_partner_inside_a_fold_is_unfolded(cx: &mut TestAppContext) {
    let (view, cx) = rust(cx, "g(|\n    a,\n    b,\n);\n");
    view.update(cx, |view, _| view.model.fold_rows(1..=3));
    run(&view, cx);
    let folds = view.update(cx, |view, _| view.model.display().folds().len());
    assert_eq!(folds, 0);
    assert_eq!(state(&view, cx), "g(\n    a,\n    b,\n|);\n");
}
