//! The kind-specific code action commands: Organize Imports, Fix All,
//! Refactor… and Source Action…, and Quick Fix leaving source actions out.

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::TestAppContext;
use lsp_types::request::{CodeActionRequest, Request};
use serde_json::json;

use super::{asked, fix, prompt, reply, setup, showing, text, titles};
use crate::tests::lsp::Lsp;

/// Runs the editor command `id` as its key would.
fn run(s: &mut Lsp, id: &str) {
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

#[gpui::test]
fn organize_imports_applies_the_one_action(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Organize imports", "kind": "source.organizeImports", "edit": fix(&s) }]),
    );
    run(&mut s, "lsp.organize_imports");
    s.until("the edit", |s| {
        text(s, &editor) == "fn main() {\n    fixed();\n}\n"
    });
    let sent = asked(&s, CodeActionRequest::METHOD);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["context"]["only"],
        json!(["source.organizeImports"])
    );
    assert!(!showing(&mut s));

    // Two of them: listed.
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([
            { "title": "Organize imports", "kind": "source.organizeImports" },
            { "title": "Sort imports", "kind": "source.organizeImports.sort" },
        ]),
    );
    run(&mut s, "lsp.organize_imports");
    s.until("the picker", showing);
    assert_eq!(titles(&mut s), ["Organize imports", "Sort imports"]);
    s.cx.simulate_keystrokes("escape");
    s.draw();

    // None: said, naming the kind.
    reply(&s, CodeActionRequest::METHOD, json!([]));
    run(&mut s, "lsp.organize_imports");
    s.until("the prompt", |s| prompt(s).is_some());
    assert_eq!(
        prompt(&mut s).as_deref(),
        Some("No organize imports actions available")
    );
    assert!(!showing(&mut s));
}

#[gpui::test]
fn quick_fix_leaves_source_actions_out(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([
            { "title": "Fix helper", "kind": "quickfix" },
            { "title": "Fix everything", "kind": "source.fixAll" },
        ]),
    );
    run(&mut s, "lsp.code_actions");
    s.until("the picker", showing);
    assert_eq!(titles(&mut s), ["Fix helper"]);
    assert_eq!(
        asked(&s, CodeActionRequest::METHOD)[0]["context"].get("only"),
        None
    );
    s.cx.simulate_keystrokes("escape");
    s.draw();

    run(&mut s, "lsp.source_action");
    s.until("the picker", |s| {
        showing(s) && titles(s) == ["Fix everything"]
    });
    let sent = asked(&s, CodeActionRequest::METHOD);
    assert_eq!(sent[1]["context"]["only"], json!(["source"]));
}

#[gpui::test]
fn refactor_lists_refactors(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([
            { "title": "Extract function", "kind": "refactor.extract" },
            { "title": "Fix helper", "kind": "quickfix" },
        ]),
    );
    run(&mut s, "lsp.refactor");
    s.until("the picker", showing);
    assert_eq!(titles(&mut s), ["Extract function"]);
    let sent = asked(&s, CodeActionRequest::METHOD);
    assert_eq!(sent[0]["context"]["only"], json!(["refactor"]));
}
