//! The actions above each conflict and the labels on its markers, in a
//! headless workbench.

use fe_command::{ContextFrame, ContextStack, Invocation, Source, Value};
use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::{EditorView, Lens, commands::RunCommand};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{TWO, dispatch, draw, open_text};

const TITLES: [&str; 3] = [
    "Accept Current Change",
    "Accept Incoming Change",
    "Accept Both Changes",
];
const COMMANDS: [&str; 3] = [
    "git.accept_current",
    "git.accept_incoming",
    "git.accept_both",
];

fn row_args(row: u32) -> Value {
    Value::Object([("row".to_owned(), row.into())].into_iter().collect())
}

/// The lenses every provider of `editor` answers for its text.
fn lenses(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Lens> {
    editor.update(cx, |editor, _| {
        let snapshot = editor.model().snapshot().clone();
        (editor.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .collect()
    })
}

fn expected(row: u32) -> Lens {
    Lens {
        row,
        items: (TITLES.iter().zip(COMMANDS))
            .map(|(title, command)| fe_gpui::LensItem {
                title: (*title).into(),
                invocation: Invocation::of(command).unwrap().args(row_args(row)),
            })
            .collect(),
    }
}

/// The buffer rows carrying an after-line `label`.
fn labelled(editor: &Entity<EditorView>, label: &str, cx: &mut VisualTestContext) -> Vec<u32> {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let text = model.text();
        (model.decorations(0..text.len()).into_iter())
            .filter(|d| matches!(&d.kind, DecorationKind::AfterLine(l) if &**l == label))
            .inspect(|d| assert_eq!(d.tint, Tint::Muted))
            .map(|d| text.offset_to_point(d.range.end).row)
            .collect()
    })
}

#[gpui::test]
fn each_conflict_has_its_actions_above_it(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    assert_eq!(lenses(&editor, cx), [expected(0), expected(6)]);
    assert_eq!(labelled(&editor, "(Current Change)", cx), [0, 6]);
    assert_eq!(labelled(&editor, "(Incoming Change)", cx), [4, 12]);
}

#[gpui::test]
fn a_lens_action_resolves_its_conflict(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    let invocation = lenses(&editor, cx)[1].items[1].invocation.clone();
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let run = RunCommand(invocation.context(context).source(Source::Mouse));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> other\nmid\ne\n");
    assert_eq!(lenses(&editor, cx), [expected(0)]);
}

#[gpui::test]
fn lenses_follow_edits(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    editor.update(cx, |editor, _| {
        editor.model_mut().select_range(0..0, false);
        editor.model_mut().insert("x\ny\n");
    });
    draw(cx);
    assert_eq!(lenses(&editor, cx), [expected(2), expected(8)]);
}

#[gpui::test]
fn accepting_splits_a_vim_insert_session(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = open_text(cx, TWO);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    cx.run_until_parked();
    let at = editor.read_with(cx, |editor, _| editor.model().text().row_start(1));
    editor.update(cx, |editor, cx| editor.select_and_reveal(at..at, cx));
    let text = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().text().to_string())
    };

    cx.simulate_keystrokes("i x");
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.vim_mode()), Some("insert"));
    let typed = text(cx);
    assert!(typed.starts_with("<<<<<<< HEAD\nxa\n"), "{typed}");

    let invocation = lenses(&editor, cx)[0].items[0].invocation.clone();
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let run = RunCommand(invocation.context(context).source(Source::Mouse));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let accepted = text(cx);
    assert!(accepted.starts_with("xa\nmid\n"), "{accepted}");

    cx.simulate_keystrokes("y");
    cx.run_until_parked();
    assert!(text(cx).starts_with("yxa\nmid\n"), "{}", text(cx));

    // Each undoes apart: the typing after, the acceptance, the typing before.
    for expected in [accepted, typed, TWO.to_owned()] {
        dispatch("editor.undo", cx);
        assert_eq!(text(cx), expected);
    }
}
