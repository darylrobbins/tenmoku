//! Typing a commit character of the selected item accepts it, then types
//! the character.

use std::rc::Rc;

use fe_buffer::{Buffer, BufferId, Version};
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionTrigger},
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{TEXT, keys};
use crate::*;

/// Offers `value`, which commits on `.`, and `vanilla`, which doesn't.
struct Provider;

impl CompletionProvider for Provider {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        Vec::new()
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        _: CompletionTrigger,
    ) -> CompletionFuture {
        let range = offset - 2..offset;
        let item = |label: &str, commit_characters: &[&str]| CompletionItem {
            label: label.into(),
            edit: (range.clone(), label.into()),
            commit_characters: commit_characters.iter().map(|c| c.to_string()).collect(),
            ..CompletionItem::default()
        };
        let items = vec![item("value", &["."]), item("vanilla", &[])];
        Box::pin(async move {
            Some(CompletionList {
                items,
                is_incomplete: false,
            })
        })
    }

    fn resolve(&self, item: CompletionItem) -> ResolveFuture {
        Box::pin(async move { item })
    }
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(TEXT.find("va\n").unwrap() + 2);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_completion_provider(Some(Rc::new(Provider)));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

/// The texts `editor.undo` steps through, until it stops changing them.
fn undone(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    let mut texts = vec![text(view, cx)];
    loop {
        keys("cmd-z", cx);
        let now = text(view, cx);
        if texts.last() == Some(&now) {
            return texts;
        }
        texts.push(now);
    }
}

#[gpui::test]
fn typing_a_commit_character_accepts_then_types_it(cx: &mut TestAppContext) {
    let at = |text: &str| TEXT.replace("va\n", &format!("{text}\n"));
    let (view, cx) = editor(cx);

    // `value` is selected: `.` accepts it, then lands after it.
    keys("ctrl-space", cx);
    cx.simulate_input(".");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), at("value."));
    let caret = view.read_with(cx, |view, _| view.model().primary_selection().head());
    assert_eq!(caret, TEXT.find("va\n").unwrap() + "value.".len());
    assert!(!view.read_with(cx, |view, _| view.completion_shown()));
    let committed = undone(&view, cx);
    assert_eq!(committed.last().unwrap(), &at("va"));

    // The same as accepting with the command, then typing it.
    let (view, cx) = editor(cx);
    keys("ctrl-space", cx);
    keys("enter", cx);
    cx.simulate_input(".");
    cx.run_until_parked();
    assert_eq!(undone(&view, cx), committed);

    // `vanilla` has none: `.` is typed over the word.
    let (view, cx) = editor(cx);
    keys("ctrl-space", cx);
    keys("down", cx);
    cx.simulate_input(".");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), at("va."));
}
