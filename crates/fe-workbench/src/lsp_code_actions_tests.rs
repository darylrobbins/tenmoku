//! Code actions from the editor: fe-lsp's fake server over a temporary
//! project (the LSP tests' [`Lsp`]), with a diagnostic over `helper` in the
//! open file.

use std::{fs, thread};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeActionProviderCapability, Diagnostic, ExecuteCommandOptions,
    Position, PublishDiagnosticsParams, Range, ServerCapabilities, TextEdit, WorkspaceEdit,
    notification::PublishDiagnostics,
    request::{ApplyWorkspaceEdit, CodeActionRequest, ExecuteCommand, Request},
};
use serde_json::{Value, json};

use super::{CodeActionsPicker, LAST};
use crate::picker::Picker;
use crate::tests::lsp::Lsp;
use crate::workbench::Palette;

const MAIN: &str = "fn main() {\n    helper();\n}\n";
const MESSAGE: &str = "unresolved name";

/// A server that offers code actions and runs `fix.all`, with `main.rs`
/// open, a diagnostic over `helper` and its caret in it.
fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut init = initialize_result(Some(lsp_types::PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["fix.all".into()],
            ..Default::default()
        }),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, script, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some())
        .unwrap();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics: vec![Diagnostic {
                range: helper(),
                message: MESSAGE.into(),
                code: Some(lsp_types::NumberOrString::String("E1".into())),
                ..Default::default()
            }],
            version: Some(0),
        });
    s.until("the diagnostic", |s| {
        editor.read_with(&s.cx, |view, _| view.model().diagnostics().len() == 1)
    });
    let caret = MAIN.find("helper").unwrap() + 2;
    s.cx.update(|window, cx| {
        editor.update(cx, |view, _| view.model_mut().set_cursor(caret));
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    (s, editor)
}

/// `helper` on `main.rs`' row 1.
fn helper() -> Range {
    Range::new(Position::new(1, 4), Position::new(1, 10))
}

/// Runs `lsp.code_actions` as the editor's keys would.
fn code_actions(s: &mut Lsp) {
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(Invocation::of("lsp.code_actions").unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

fn reply(s: &Lsp, method: &str, result: Value) {
    s.fake
        .set_reply(method, Scripted::now(Reply::Result(result)));
}

fn asked(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

fn picker() -> Entity<Picker<CodeActionsPicker>> {
    LAST.with(|last| last.borrow().clone())
        .and_then(|picker| picker.upgrade())
        .expect("a code actions picker was shown")
}

fn showing(s: &mut Lsp) -> bool {
    s.workbench
        .read_with(&s.cx, |w, _| w.palette == Some(Palette::CodeActions))
}

/// The rows' titles, kinds and reasons, the selected row and the status.
type Rows = Vec<(String, Option<String>, Option<String>)>;

fn state(s: &mut Lsp) -> (Rows, usize, String) {
    picker().read_with(&s.cx, |picker, _| {
        (
            picker.delegate.rows(),
            picker.selected(),
            picker.delegate.status_text(),
        )
    })
}

fn titles(s: &mut Lsp) -> Vec<String> {
    state(s).0.into_iter().map(|row| row.0).collect()
}

fn text(s: &mut Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn prompt(s: &mut Lsp) -> Option<String> {
    s.cx.pending_prompt().map(|(question, _)| question)
}

/// An edit of `main.rs` renaming `helper` to `fixed`.
fn fix(s: &Lsp) -> Value {
    json!({ "changes": { s.uri("src/main.rs").as_str(): [{
        "range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 10}},
        "newText": "fixed",
    }]}})
}

fn diagnostic() -> Value {
    json!({ "range": helper(), "message": MESSAGE, "code": "E1" })
}

#[gpui::test]
fn quick_fix_applies(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    // The server's order: a refactor, then the quick fixes, the preferred one
    // that fixes nothing last of them.
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([
            { "title": "Extract function", "kind": "refactor.extract" },
            { "title": "Add an import", "kind": "quickfix" },
            { "title": "Fix all", "kind": "quickfix", "isPreferred": true },
            { "title": "Other fix", "kind": "quickfix", "diagnostics": [diagnostic()] },
            { "title": "Fix helper", "kind": "quickfix", "isPreferred": true,
              "diagnostics": [diagnostic()], "edit": fix(&s) },
        ]),
    );

    code_actions(&mut s);
    s.until("the picker", showing);
    // The diagnostic went with the request, over the caret.
    let sent = asked(&s, CodeActionRequest::METHOD);
    assert_eq!(sent.len(), 1);
    let caret = json!({"line": 1, "character": 6});
    assert_eq!(sent[0]["range"], json!({"start": caret, "end": caret}));
    let diagnostics = sent[0]["context"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["message"], MESSAGE);
    assert_eq!(diagnostics[0]["code"], "E1");

    // The fix for the diagnostic first, selected.
    let (rows, selected, _) = state(&mut s);
    assert_eq!(
        rows,
        [
            ("Fix helper".to_owned(), Some("quickfix".to_owned()), None),
            ("Fix all".to_owned(), Some("quickfix".to_owned()), None),
            ("Other fix".to_owned(), Some("quickfix".to_owned()), None),
            (
                "Add an import".to_owned(),
                Some("quickfix".to_owned()),
                None
            ),
            (
                "Extract function".to_owned(),
                Some("refactor.extract".to_owned()),
                None
            ),
        ]
    );
    assert_eq!(selected, 0);

    // Typing filters by subsequence.
    s.cx.simulate_input("EXF");
    s.draw();
    assert_eq!(titles(&mut s), ["Extract function"]);
    s.cx.simulate_keystrokes("backspace backspace backspace");
    s.draw();
    assert_eq!(titles(&mut s).len(), 5);

    s.cx.simulate_keystrokes("enter");
    s.until("the edit", |s| !showing(s));
    let edited = "fn main() {\n    fixed();\n}\n";
    assert_eq!(text(&mut s, &editor), edited);
    assert!(s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // One undo.
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&mut s, &editor), MAIN);
}

#[gpui::test]
fn commands_and_disabled(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([
            { "title": "Fix everything", "kind": "quickfix",
              "command": { "title": "Fix everything", "command": "fix.all" } },
            { "title": "Inline", "kind": "refactor.inline",
              "disabled": { "reason": "nothing to inline" } },
        ]),
    );
    // The command answers after a while, the edit it applies sent first.
    let delay = std::time::Duration::from_millis(500);
    let answered = Scripted {
        reply: Reply::Result(Value::Null),
        delay,
    };
    s.fake.set_reply(ExecuteCommand::METHOD, answered);

    code_actions(&mut s);
    s.until("the picker", showing);
    let (rows, _, _) = state(&mut s);
    assert_eq!(rows[1].2.as_deref(), Some("nothing to inline"));
    assert_eq!(rows[1].1.as_deref(), Some("refactor.inline"));

    // A disabled one can't be picked: the picker stays, saying why.
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    assert!(showing(&mut s));
    assert_eq!(state(&mut s).2, "Disabled: nothing to inline");
    assert!(asked(&s, ExecuteCommand::METHOD).is_empty());

    // The command runs, and the edit the server sends meanwhile is applied.
    s.cx.simulate_keystrokes("up enter");
    s.until("the command", |s| {
        !asked(s, ExecuteCommand::METHOD).is_empty()
    });
    assert_eq!(asked(&s, ExecuteCommand::METHOD)[0]["command"], "fix.all");
    let params = ApplyWorkspaceEditParams {
        label: Some("Fix everything".into()),
        edit: WorkspaceEdit::new(
            [(
                s.uri("src/main.rs"),
                vec![TextEdit::new(helper(), "fixed".into())],
            )]
            .into(),
        ),
    };
    let fake = s.fake.clone();
    let pushed = thread::spawn(move || fake.request::<ApplyWorkspaceEdit>(params).unwrap());
    s.until("the edit", |s| text(s, &editor).contains("fixed"));
    s.until("the picker to close", |s| !showing(s));
    assert_eq!(pushed.join().unwrap()["applied"], true);
    assert!(s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn empty_and_stale(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);

    reply(&s, CodeActionRequest::METHOD, json!([]));
    code_actions(&mut s);
    s.until("the prompt", |s| prompt(s).is_some());
    assert_eq!(prompt(&mut s).as_deref(), Some("No code actions available"));
    assert!(!showing(&mut s));
    s.cx.simulate_prompt_answer("OK");
    s.draw();

    // Typing in the buffer before the pick refuses it.
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Fix helper", "kind": "quickfix", "edit": fix(&s) }]),
    );
    code_actions(&mut s);
    s.until("the picker", showing);
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert("x")));
    s.draw();
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert_eq!(
        prompt(&mut s).as_deref(),
        Some("The file changed; ask again")
    );
    assert!(!showing(&mut s));
    assert!(!text(&mut s, &editor).contains("fixed"));
    s.cx.simulate_prompt_answer("OK");
    s.draw();
}
