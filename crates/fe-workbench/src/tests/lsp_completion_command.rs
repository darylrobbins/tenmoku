//! An accepted completion's command, run by its server
//! (`workspace/executeCommand`), whose edit lands with the completion.

use std::{fs, thread};

use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{
    ApplyWorkspaceEditParams, CompletionOptions, ExecuteCommandOptions, Position, Range,
    ServerCapabilities, TextEdit, WorkspaceEdit,
    request::{ApplyWorkspaceEdit, Completion, ExecuteCommand, Request as _},
};
use serde_json::{Value, json};

use super::super::lsp::Lsp;

const MAIN: &str = "fn main() {\n    fo\n}\n";

#[gpui::test]
fn accepting_a_completion_runs_its_server_command(cx: &mut TestAppContext) {
    let mut init = initialize_result(Some(lsp_types::PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        completion_provider: Some(CompletionOptions::default()),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["add.import".into()],
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
    let reply = json!({
        "isIncomplete": false,
        "items": [{
            "label": "foo",
            "textEdit": {
                "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 6 } },
                "newText": "foo"
            },
            "command": { "title": "Add import", "command": "add.import", "arguments": ["foo"] }
        }]
    });
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(reply)));
    s.fake.set_reply(
        ExecuteCommand::METHOD,
        Scripted::now(Reply::Result(Value::Null)),
    );
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open))
        .unwrap();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(MAIN.find("fo\n").unwrap() + 2);
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();

    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_keystrokes("enter");
    let executed = |s: &Lsp| {
        (s.fake.records().iter())
            .filter_map(|record| match record {
                Record::Request { method, params, .. } if method == ExecuteCommand::METHOD => {
                    Some(params.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    s.until("the command", |s| !executed(s).is_empty());
    let sent = executed(&s);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["command"], "add.import");
    assert_eq!(sent[0]["arguments"], json!(["foo"]));

    // The edit the server then pushes lands with the completion.
    let params = ApplyWorkspaceEditParams {
        label: Some("Add import".into()),
        edit: WorkspaceEdit::new(
            [(
                main,
                vec![TextEdit::new(
                    Range::new(Position::new(0, 0), Position::new(0, 0)),
                    "use foo;\n".into(),
                )],
            )]
            .into(),
        ),
    };
    let fake = s.fake.clone();
    let pushed = thread::spawn(move || fake.request::<ApplyWorkspaceEdit>(params).unwrap());
    let text = |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    s.until("the import", |s| text(s).starts_with("use foo;"));
    assert_eq!(text(&mut s), "use foo;\nfn main() {\n    foo\n}\n");
    assert_eq!(pushed.join().unwrap()["applied"], true);
}
