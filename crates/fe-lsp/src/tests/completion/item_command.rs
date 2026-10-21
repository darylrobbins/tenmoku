//! An item's `command`: a server one runs through `workspace/executeCommand`,
//! a client one or an unlisted one sends nothing, and the client lists its
//! own commands for rust-analyzer.

use std::collections::HashMap;
use std::path::Path;

use fe_buffer::Buffer;
use lsp_types::request::{Completion, ExecuteCommand, Request, ResolveCompletionItem};
use lsp_types::{CompletionOptions, ExecuteCommandOptions, PositionEncodingKind};
use serde_json::{Value, json};

use super::super::{Started, answer, manager};
use crate::fake::{FakeServer, InitializeReply, Reply, Script, Scripted, initialize_result};
use crate::{CompletionTrigger, LspError, ServerSpec};

/// A server that lists `fix.import` and resolves, started on the fake.
fn started() -> Started {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities.completion_provider = Some(CompletionOptions {
        resolve_provider: Some(true),
        ..CompletionOptions::default()
    });
    init.capabilities.execute_command_provider = Some(ExecuteCommandOptions {
        commands: vec!["fix.import".into()],
        ..Default::default()
    });
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Result(Box::new(init)),
        replies: HashMap::new(),
        ..Script::default()
    });
    let (mut manager, events) = manager(fake.launcher(), None);
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            Path::new(super::super::ROOT),
        )
        .unwrap();
    fake.wait_initialized();
    Started {
        fake,
        manager,
        server,
        events,
    }
}

/// The items `listed` gives, asked for in a fresh buffer.
fn items(s: &mut Started, listed: Value) -> Vec<crate::CompletionItem> {
    let mut buffer = Buffer::new("va");
    s.open_synced(&mut buffer, "a.rs");
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(listed)));
    answer(
        s.manager
            .completion(buffer.id(), buffer.version(), 2, CompletionTrigger::Invoked),
    )
    .unwrap()
    .unwrap()
    .items
}

/// The params of each `workspace/executeCommand` the fake got.
fn executed(s: &Started) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            crate::fake::Record::Request { method, params, .. }
                if method == ExecuteCommand::METHOD =>
            {
                Some(params)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_listed_command_runs_through_execute_command() {
    let mut s = started();
    s.fake.set_reply(
        ExecuteCommand::METHOD,
        Scripted::now(Reply::Result(Value::Null)),
    );
    let mut items = items(
        &mut s,
        json!([
            {
                "label": "a",
                "command": { "title": "t", "command": "fix.import", "arguments": [1, "a"] },
            },
            { "label": "b" },
        ]),
    );
    let plain = items.pop().unwrap();
    let item = items.pop().unwrap();
    assert_eq!(item.command().unwrap().command, "fix.import");
    assert!(plain.command().is_none());
    answer(s.manager.run_completion_command(&item)).unwrap();
    let sent = executed(&s);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["command"], "fix.import");
    assert_eq!(sent[0]["arguments"], json!([1, "a"]));

    // A resolve that adds the command.
    s.fake.set_reply(
        ResolveCompletionItem::METHOD,
        Scripted::now(Reply::Result(json!({
            "label": "b",
            "command": { "title": "t", "command": "fix.import", "arguments": [2] },
        }))),
    );
    let resolved = answer(s.manager.resolve(plain)).unwrap();
    assert_eq!(resolved.command().unwrap().command, "fix.import");
    answer(s.manager.run_completion_command(&resolved)).unwrap();
    let sent = executed(&s);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1]["command"], "fix.import");
    assert_eq!(sent[1]["arguments"], json!([2]));
}

#[test]
fn client_and_unlisted_commands_send_nothing() {
    let mut s = started();
    let items = items(
        &mut s,
        json!([
            { "label": "a", "command": { "title": "t", "command": "editor.action.triggerParameterHints" } },
            { "label": "b", "command": { "title": "t", "command": "other.cmd" } },
            { "label": "c" },
        ]),
    );
    for item in &items[..2] {
        assert!(matches!(
            answer(s.manager.run_completion_command(item)),
            Err(LspError::Unavailable(_))
        ));
    }
    assert!(answer(s.manager.run_completion_command(&items[2])).is_ok());
    assert!(executed(&s).is_empty());
}

#[test]
fn experimental_commands_are_advertised() {
    let s = started();
    let init = (s.fake.records().into_iter())
        .find_map(|record| match record {
            crate::fake::Record::Request { method, params, .. } if method == "initialize" => {
                Some(params)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        init["capabilities"]["experimental"],
        json!({
            "serverStatusNotification": true,
            "commands": { "commands": [
                "editor.action.triggerParameterHints",
                "editor.action.triggerSuggest",
            ] },
        })
    );
}
