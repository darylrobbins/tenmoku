//! Accepting an item runs its command after the edit: the editor's own
//! (`editor.action.triggerSuggest`, `editor.action.triggerParameterHints`)
//! itself, any other through the provider.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, BufferId, Version};
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionTrigger},
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{TEXT, keys};
use crate::{
    signature_help::{Signature, SignatureContext, SignatureHelp},
    *,
};

/// Offers one item per command (and one with none), recording each request
/// and each command it is handed, with the text it found.
#[derive(Default)]
struct Provider {
    asked: RefCell<Vec<(usize, CompletionTrigger)>>,
    ran: RefCell<Vec<CompletionItem>>,
    command: RefCell<Option<String>>,
}

impl CompletionProvider for Provider {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        Vec::new()
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> CompletionFuture {
        self.asked.borrow_mut().push((offset, trigger));
        let item = CompletionItem {
            label: "value".into(),
            edit: (offset - 2..offset, "value".into()),
            command: self.command.borrow().clone(),
            commit_characters: vec![".".into()],
            ..CompletionItem::default()
        };
        Box::pin(async move {
            Some(CompletionList {
                items: vec![item],
                is_incomplete: false,
            })
        })
    }

    fn resolve(&self, item: CompletionItem) -> ResolveFuture {
        Box::pin(async move { item })
    }

    fn run_command(&self, item: CompletionItem) {
        self.ran.borrow_mut().push(item);
    }
}

struct Signatures;

impl SignatureHelpProvider for Signatures {
    fn triggers(&self, _: BufferId) -> Option<(Vec<char>, Vec<char>)> {
        None
    }

    fn signature_help(
        &self,
        _: BufferId,
        _: Version,
        _: usize,
        _: SignatureContext,
    ) -> SignatureHelpFuture {
        let help = SignatureHelp {
            signatures: vec![Signature {
                label: "value(a: i32)".into(),
                documentation: None,
                parameters: Vec::new(),
                active_parameter: None,
            }],
            active_signature: 0,
            active_parameter: None,
        };
        Box::pin(async move { Some(help) })
    }
}

/// A focused editor on [`TEXT`], the caret after `va`, whose one item has
/// `command`.
fn editor<'a>(
    command: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, Rc<Provider>, &'a mut VisualTestContext) {
    cx.update(init);
    let provider = Rc::new(Provider::default());
    *provider.command.borrow_mut() = command.map(String::from);
    let set = provider.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(TEXT.find("va\n").unwrap() + 2);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_completion_provider(Some(set));
        view.set_signature_help_provider(Some(Rc::new(Signatures)));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, provider, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

#[gpui::test]
fn accepting_runs_the_items_command(cx: &mut TestAppContext) {
    let word = TEXT.find("va\n").unwrap();

    // Asking again, at the caret the edit left.
    let (view, provider, cx) = editor(Some("editor.action.triggerSuggest"), cx);
    keys("ctrl-space", cx);
    keys("enter", cx);
    assert_eq!(
        *provider.asked.borrow(),
        [
            (word + 2, CompletionTrigger::Invoked),
            (word + 5, CompletionTrigger::Invoked)
        ]
    );
    assert!(provider.ran.borrow().is_empty());

    // Signature help, as `editor.show_signature_help` asks.
    let (view_hints, provider, cx) = editor(Some("editor.action.triggerParameterHints"), cx);
    keys("ctrl-space", cx);
    keys("enter", cx);
    let label = view_hints.read_with(cx, |view, _| {
        view.signature_help
            .painted
            .as_ref()
            .map(|p| p.label.clone())
    });
    assert_eq!(label.as_deref(), Some("value(a: i32)"));
    assert_eq!(provider.asked.borrow().len(), 1);
    assert!(provider.ran.borrow().is_empty());
    let _ = view;

    // Another id goes to the provider, once, with the text in.
    let (view, provider, cx) = editor(Some("x.cmd"), cx);
    keys("ctrl-space", cx);
    keys("enter", cx);
    assert_eq!(text(&view, cx), TEXT.replace("va\n", "value\n"));
    let ran = provider.ran.borrow();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].label, "value");
    assert_eq!(ran[0].command.as_deref(), Some("x.cmd"));
    drop(ran);
    assert_eq!(provider.asked.borrow().len(), 1);

    // None runs nothing.
    let (view, provider, cx) = editor(None, cx);
    keys("ctrl-space", cx);
    keys("enter", cx);
    assert_eq!(text(&view, cx), TEXT.replace("va\n", "value\n"));
    assert!(provider.ran.borrow().is_empty());
    assert_eq!(provider.asked.borrow().len(), 1);
}

/// A commit character types after the accept, and the command then asks at
/// the caret that leaves, with no trigger character to ask for it.
#[gpui::test]
fn a_commit_character_runs_the_command_after_it_is_typed(cx: &mut TestAppContext) {
    let word = TEXT.find("va\n").unwrap();
    let (view, provider, cx) = editor(Some("editor.action.triggerSuggest"), cx);
    keys("ctrl-space", cx);
    cx.simulate_input(".");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), TEXT.replace("va\n", "value.\n"));
    assert_eq!(
        provider.asked.borrow().last(),
        Some(&(word + 6, CompletionTrigger::Invoked))
    );
    assert_eq!(provider.asked.borrow().len(), 2);
}
