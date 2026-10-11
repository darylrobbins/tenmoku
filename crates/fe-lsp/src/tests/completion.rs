//! `textDocument/completion` and `completionItem/resolve` (§7,
//! "Completion"): items as byte edits in the version asked about, what
//! the client advertises, resolve, the trigger characters, and the 3 s and
//! 2 s timeouts.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{Completion, Request, ResolveCompletionItem};
use lsp_types::{
    CompletionItemKind, CompletionOptions, NumberOrString, Position, PositionEncodingKind,
};
use serde_json::{Value, json};

use super::{Started, answer, manager};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{
    CompletionEdit, CompletionTrigger, Completions, ManualClock, ServerSpec, ServerStatus,
};

/// The LSP position of byte `offset` of `text`, counted as the protocol
/// says, not with fe-lsp's converter.
fn position(text: &str, offset: usize, utf16: bool) -> Value {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
    let character = if utf16 {
        column.encode_utf16().count()
    } else {
        column.len()
    };
    serde_json::to_value(Position::new(line as u32, character as u32)).unwrap()
}

/// The requests for `method` the fake got: each one's id and params.
fn requests(records: &[Record], method: &str) -> Vec<(NumberOrString, Value)> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request {
                id,
                method: m,
                params,
            } if m == method => Some((id.clone(), params.clone())),
            _ => None,
        })
        .collect()
}

/// A server in `encoding` that completes on `.` and `:` and resolves,
/// started on the fake, with the manager's clock.
fn started(encoding: PositionEncodingKind) -> (Started, Arc<ManualClock>) {
    let mut init = initialize_result(Some(encoding));
    init.capabilities.completion_provider = Some(CompletionOptions {
        resolve_provider: Some(true),
        trigger_characters: Some(vec![".".into(), ":".into()]),
        ..CompletionOptions::default()
    });
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Result(Box::new(init)),
        replies: HashMap::new(),
        ..Script::default()
    });
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            Path::new(super::ROOT),
        )
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    let s = Started {
        fake,
        manager,
        server,
        events,
    };
    (s, clock)
}

fn edit(range: std::ops::Range<usize>, text: &str) -> CompletionEdit {
    CompletionEdit {
        range,
        text: text.into(),
    }
}

#[test]
fn items_convert_to_byte_edits() {
    for (encoding, utf16) in [
        (PositionEncodingKind::UTF8, false),
        (PositionEncodingKind::UTF16, true),
    ] {
        let (mut s, _) = started(encoding);

        // The client's capabilities.
        let init = (s.fake.records().into_iter())
            .find_map(|record| match record {
                Record::Request { method, params, .. } if method == "initialize" => Some(params),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            init["capabilities"]["textDocument"]["completion"]["completionItem"],
            json!({
                "snippetSupport": false,
                "insertReplaceSupport": false,
                "resolveSupport": { "properties": ["documentation", "detail"] },
                "labelDetailsSupport": true,
            })
        );

        // An emoji earlier on the line: two UTF-16 units, four bytes.
        let v0 = "use std;\nlet 🦀 = val.fo\n";
        let fo = v0.find("fo\n").unwrap();
        let end = fo + 2;
        let mut buffer = Buffer::new(v0);
        let uri = s.open_synced(&mut buffer, "a.rs");
        let fo_range = json!({ "start": position(v0, fo, utf16), "end": position(v0, end, utf16) });
        let top = json!({ "start": position(v0, 0, utf16), "end": position(v0, 0, utf16) });
        s.fake.set_reply(
            Completion::METHOD,
            Scripted::now(Reply::Result(json!({
                "isIncomplete": true,
                "items": [
                    {
                        "label": "foo",
                        "labelDetails": { "detail": "(x: i32)", "description": "crate::foo" },
                        "kind": 3,
                        "detail": "fn foo(x: i32)",
                        "documentation": { "kind": "markdown", "value": "Does *foo*." },
                        "filterText": "foo",
                        "sortText": "0001",
                        "textEdit": { "range": fo_range, "newText": "foo()" },
                        "additionalTextEdits": [
                            { "range": top, "newText": "use crate::foo;\n" },
                        ],
                    },
                    { "label": "format", "insertText": "format!", "kind": 15 },
                    { "label": "forget" },
                ],
            }))),
        );
        // Asked at the end of `fo`, in the session's encoding, with the
        // trigger.
        let completions = answer(s.manager.completion(
            buffer.id(),
            buffer.version(),
            end,
            CompletionTrigger::Invoked,
        ))
        .unwrap()
        .unwrap();
        let (_, params) = requests(&s.fake.records(), Completion::METHOD)
            .pop()
            .unwrap();
        assert_eq!(params["textDocument"]["uri"], json!(uri));
        assert_eq!(params["position"], position(v0, end, utf16));
        assert_eq!(params["context"], json!({ "triggerKind": 1 }));

        assert!(completions.is_incomplete);
        let items = &completions.items;
        assert_eq!(items.len(), 3);
        let foo = &items[0];
        assert_eq!(foo.label, "foo");
        assert_eq!(foo.label_detail.as_deref(), Some("(x: i32)"));
        assert_eq!(foo.label_description.as_deref(), Some("crate::foo"));
        assert_eq!(foo.kind, Some(CompletionItemKind::FUNCTION));
        assert_eq!(crate::kind_name(CompletionItemKind::FUNCTION), "function");
        assert_eq!(foo.detail.as_deref(), Some("fn foo(x: i32)"));
        assert_eq!(foo.documentation.as_deref(), Some("Does *foo*."));
        assert_eq!(foo.filter_text.as_deref(), Some("foo"));
        assert_eq!(foo.sort_text.as_deref(), Some("0001"));
        assert_eq!(foo.edit, edit(fo..end, "foo()"));
        assert_eq!(foo.additional_edits, [edit(0..0, "use crate::foo;\n")]);
        // Only `insertText`, or only a label: over the word before the
        // offset.
        assert_eq!(items[1].edit, edit(fo..end, "format!"));
        assert_eq!(items[1].kind, Some(CompletionItemKind::SNIPPET));
        assert_eq!(items[2].edit, edit(fo..end, "forget"));
        assert_eq!(items[2].additional_edits, []);

        // A bare array is complete; ranges are in the version asked
        // about, though the buffer has moved on by the reply.
        s.fake.set_reply(
            Completion::METHOD,
            Scripted {
                reply: Reply::Result(json!([
                    { "label": "foo", "textEdit": { "range": fo_range, "newText": "foo" } },
                ])),
                delay: Duration::from_secs(1),
            },
        );
        // Count before asking: the request can reach the fake first.
        let count = requests(&s.fake.records(), Completion::METHOD).len();
        let asked = s.manager.completion(
            buffer.id(),
            buffer.version(),
            end,
            CompletionTrigger::Character(".".into()),
        );
        let records = s.fake.wait_for("the completion", |records| {
            requests(records, Completion::METHOD).len() > count
        });
        let (_, params) = requests(&records, Completion::METHOD).pop().unwrap();
        assert_eq!(
            params["context"],
            json!({ "triggerKind": 2, "triggerCharacter": "." })
        );
        s.edit(&mut buffer, &uri, (0..0, "\n"), 1);
        let answered: Completions = answer(asked).unwrap().unwrap();
        assert!(!answered.is_incomplete);
        assert_eq!(answered.items[0].edit, edit(fo..end, "foo"));
    }
}

#[test]
fn resolve_triggers_and_timeouts() {
    let (mut s, clock) = started(PositionEncodingKind::UTF16);
    let text = "let x = va\n";
    let mut buffer = Buffer::new(text);
    // Its trigger characters, once it's open on the server.
    assert_eq!(
        s.manager.trigger_characters(buffer.id()),
        Vec::<String>::new()
    );
    let uri = s.open_synced(&mut buffer, "a.rs");
    assert_eq!(s.manager.trigger_characters(buffer.id()), [".", ":"]);

    s.fake.set_reply(
        Completion::METHOD,
        Scripted::now(Reply::Result(json!([
            { "label": "value", "data": { "id": 7 } },
        ]))),
    );
    let item = answer(s.manager.completion(
        buffer.id(),
        buffer.version(),
        text.len() - 1,
        CompletionTrigger::Invoked,
    ))
    .unwrap()
    .unwrap()
    .items
    .remove(0);
    assert_eq!(
        (item.detail.as_ref(), item.documentation.as_ref()),
        (None, None)
    );

    // Resolve fills in its documentation and detail, sending the item as
    // the server gave it.
    s.fake.set_reply(
        ResolveCompletionItem::METHOD,
        Scripted::now(Reply::Result(json!({
            "label": "value",
            "detail": "let value: u8",
            "documentation": "The value.",
        }))),
    );
    let resolved = answer(s.manager.resolve(item.clone())).unwrap();
    let (_, params) = requests(&s.fake.records(), ResolveCompletionItem::METHOD)
        .pop()
        .unwrap();
    assert_eq!(params, json!({ "label": "value", "data": { "id": 7 } }));
    assert_eq!(resolved.detail.as_deref(), Some("let value: u8"));
    assert_eq!(resolved.documentation.as_deref(), Some("The value."));
    assert_eq!(resolved.edit, item.edit);

    // A resolve past `lsp.timeouts.resolve`'s 2 s: the item unchanged,
    // and the request cancelled.
    s.fake.set_reply(
        ResolveCompletionItem::METHOD,
        Scripted {
            reply: Reply::Result(json!({ "label": "value", "detail": "late" })),
            delay: Duration::from_secs(60),
        },
    );
    expect_timeout(
        &s,
        &clock,
        ResolveCompletionItem::METHOD,
        2,
        || s.manager.resolve(item.clone()).boxed(),
        Ok(item.clone()),
    );

    // A completion past `lsp.timeouts.completion`'s 3 s: none, and the
    // request cancelled.
    s.fake.set_reply(
        Completion::METHOD,
        Scripted {
            reply: Reply::Result(json!([{ "label": "late" }])),
            delay: Duration::from_secs(60),
        },
    );
    expect_timeout(
        &s,
        &clock,
        Completion::METHOD,
        3,
        || {
            (s.manager)
                .completion(buffer.id(), buffer.version(), 0, CompletionTrigger::Invoked)
                .map(Result::unwrap)
                .boxed()
        },
        None,
    );

    // Nor after the document is closed and opened again in the same
    // session.
    s.manager.close(buffer.id());
    s.fake
        .wait_for_document(&uri, "didClose", |r| r.is_some_and(|r| !r.open));
    s.open_synced(&mut buffer, "a.rs");
    let count = requests(&s.fake.records(), ResolveCompletionItem::METHOD).len();
    assert_eq!(answer(s.manager.resolve(item.clone())), Ok(item.clone()));
    s.barrier();
    assert_eq!(
        requests(&s.fake.records(), ResolveCompletionItem::METHOD).len(),
        count
    );

    // After a restart, the old session's item isn't sent to the new one,
    // whose ids may name other items: it stays as it was.
    s.fake.crash();
    while !matches!(s.events.status(s.server), ServerStatus::Restarting(_)) {}
    clock.wait_for_sleep(Duration::from_secs(1));
    clock.advance(Duration::from_secs(1));
    s.fake.wait_initialized_at(2);
    s.fake
        .wait_for_document(&uri, "reopened", |r| r.is_some_and(|r| r.launch == 2));
    assert_eq!(answer(s.manager.resolve(item.clone())), Ok(item));
    let resolves = (s.fake.records_of(2).iter())
        .filter(|record| record.method() == ResolveCompletionItem::METHOD)
        .count();
    assert_eq!(resolves, 0);
}

/// Starts `late` with `start`, waits for its `method` request, then moves the clock to just
/// short of `secs` (still waiting) and on to it: `late` answers `then`, and
/// the request is cancelled.
fn expect_timeout<T: PartialEq + std::fmt::Debug>(
    s: &Started,
    clock: &ManualClock,
    method: &str,
    secs: u64,
    start: impl FnOnce() -> futures::future::BoxFuture<'static, T>,
    then: T,
) {
    // Count before `start`: the request may go out as soon as it's built,
    // and under load the fake can record it before we'd look.
    let count = requests(&s.fake.records(), method).len();
    let mut late = start();
    assert!(late.as_mut().now_or_never().is_none());
    let records = s
        .fake
        .wait_for(method, |records| requests(records, method).len() > count);
    let (id, _) = requests(&records, method).pop().unwrap();
    let timeout = Duration::from_secs(secs);
    clock.wait_for_sleep(timeout);
    clock.advance(timeout - Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(50));
    assert!(late.as_mut().now_or_never().is_none());
    clock.advance(Duration::from_millis(1));
    assert_eq!(answer(late), then);
    s.fake.wait_for("$/cancelRequest", |records| {
        records.iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == json!(id))
        })
    });
}
