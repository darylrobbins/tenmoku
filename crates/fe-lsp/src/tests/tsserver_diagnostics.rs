//! Diagnostics pulled through `typescript.tsserverRequest` (tsserver.rs):
//! the three requests and where they go, how their replies are decoded,
//! and the built-in specs that pull this way.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fe_buffer::{Buffer, Origin, Version};
use lsp_types::notification::PublishDiagnostics;
use lsp_types::{ExecuteCommandOptions, PositionEncodingKind, PublishDiagnosticsParams, Url};
use serde_json::{Value, json};

use super::{Events, ROOT, answer, manager};
use crate::fake::{
    Echo, FakeServer, InitializeReply, Record, Reply, Script, Scripted, Writes, initialize_result,
};
use crate::registry;
use crate::{
    Diagnostic, DiagnosticTag, LspEvent, LspManager, ManualClock, PullDiagnostics, ServerId,
    ServerSpec, Severity, UnversionedDiagnostics,
};

const EXECUTE: &str = "workspace/executeCommand";
const COMMANDS: [&str; 3] = [
    "syntacticDiagnosticsSync",
    "semanticDiagnosticsSync",
    "suggestionDiagnosticsSync",
];

/// A server on the fake, with a spec pulling through tsserver, in a
/// session that negotiated `encoding`.
struct Tsserver {
    fake: FakeServer,
    manager: LspManager,
    server: ServerId,
    events: Events,
}

impl Tsserver {
    fn new(encoding: Option<PositionEncodingKind>) -> Self {
        let mut result = initialize_result(encoding);
        result.capabilities.execute_command_provider = Some(ExecuteCommandOptions {
            commands: vec![
                "_typescript.organizeImports".into(),
                "typescript.tsserverRequest".into(),
            ],
            ..Default::default()
        });
        let script = Script {
            initialize: InitializeReply::Result(Box::new(result)),
            ..Script::default()
        };
        let fake = FakeServer::with_clock(script, Arc::new(ManualClock::new()));
        let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
        let spec = ServerSpec {
            pull_diagnostics: PullDiagnostics::TsserverRequest,
            ..ServerSpec::new("fake", "fake-server")
        };
        let server = manager.start(spec, Path::new(ROOT)).unwrap();
        fake.wait_initialized();
        Self {
            fake,
            manager,
            server,
            events,
        }
    }

    /// Opens `buffer` as `name` under the root, without waiting.
    fn open(&mut self, buffer: &mut Buffer, name: &str) -> Url {
        let path = PathBuf::from(ROOT).join(name);
        let subscription = buffer.subscribe();
        self.manager
            .open_on(
                self.server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                "typescript",
            )
            .unwrap();
        Url::from_file_path(&path).unwrap()
    }

    /// Edits `buffer` and nudges its server, without waiting.
    fn edit(&self, buffer: &mut Buffer, edit: (std::ops::Range<usize>, &str)) {
        buffer.edit([edit], Origin::User).unwrap();
        self.manager.changed(buffer.id());
    }

    /// With the client's writes paced from before whatever triggers a pull,
    /// lets them through one message at a time, so the next three
    /// tsserver requests get `replies` in order, and calls `between(n)`
    /// once the fake has the `n`th of them. Writes stay paced, so nothing
    /// sent after them is read until the test opens them.
    fn step(&self, replies: [Reply; 3], mut between: impl FnMut(usize)) {
        for (n, reply) in replies.into_iter().enumerate() {
            self.fake.set_reply(EXECUTE, Scripted::now(reply));
            let want = executes(&self.fake.records()).len() + 1;
            while executes(&self.fake.records()).len() < want {
                let sent = client_messages(&self.fake.records());
                // async-lsp writes a message as its header, then its body.
                self.fake.allow_write();
                self.fake.allow_write();
                self.fake.wait_for("the next message", |records| {
                    client_messages(records) > sent
                });
            }
            between(n + 1);
        }
    }

    /// Returns once the fake has read everything the actor sent before
    /// now.
    fn barrier(&self) {
        answer(self.manager.request::<Echo>(self.server, json!({}))).unwrap();
    }
}

/// How many messages the client has sent the fake.
fn client_messages(records: &[Record]) -> usize {
    (records.iter())
        .filter(|record| {
            matches!(
                record,
                Record::Request { .. } | Record::Notification { .. } | Record::Response { .. }
            )
        })
        .count()
}

/// The params of each `workspace/executeCommand` the fake got, in order.
fn executes(records: &[Record]) -> Vec<Value> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == EXECUTE => Some(params.clone()),
            _ => None,
        })
        .collect()
}

/// What the fake got for `uri`, in order: each document notification's
/// method without `textDocument/`, and each tsserver request's command.
fn sequence(fake: &FakeServer, uri: &Url) -> Vec<String> {
    (fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Notification { method, params }
                if params["textDocument"]["uri"] == json!(uri) =>
            {
                Some(method["textDocument/".len()..].to_owned())
            }
            Record::Request { method, params, .. }
                if method == EXECUTE && params["arguments"][1]["file"] == json!(uri) =>
            {
                Some(params["arguments"][0].as_str().unwrap().to_owned())
            }
            _ => None,
        })
        .collect()
}

/// tsserver's 1-based `{ line, offset }` of byte `at` in `text`, the
/// offset in UTF-16 code units.
fn location(text: &str, at: usize) -> Value {
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |n| n + 1);
    json!({
        "line": before.matches('\n').count() + 1,
        "offset": before[line_start..].encode_utf16().count() + 1,
    })
}

/// A tsserver diagnostic over the first `word` in `text`.
fn over(text: &str, word: &str, category: &str, code: u32, extra: Value) -> Value {
    let start = text.find(word).expect("the word is in the text");
    let mut entry = json!({
        "start": location(text, start),
        "end": location(text, start + word.len()),
        "text": format!("{category} {code}"),
        "code": code,
        "category": category,
    });
    for (key, value) in extra.as_object().unwrap() {
        entry[key] = value.clone();
    }
    entry
}

/// tsserver's response to `command`, holding `body`.
fn response(command: &str, body: Vec<Value>) -> Reply {
    Reply::Result(json!({
        "seq": 0,
        "type": "response",
        "command": command,
        "request_seq": 1,
        "success": true,
        "body": body,
    }))
}

/// Three responses, the `n`th to `COMMANDS[n]`.
fn responses(bodies: [Vec<Value>; 3]) -> [Reply; 3] {
    let mut bodies = bodies.into_iter();
    COMMANDS.map(|command| response(command, bodies.next().unwrap()))
}

/// What a [`LspEvent::Diagnostics`] carried.
struct Got {
    version: Version,
    items: Vec<Diagnostic>,
}

/// The next diagnostics the sink gets.
fn next(events: &Events) -> Got {
    events.until("diagnostics", |event| match event {
        LspEvent::Diagnostics { version, items, .. } => Some(Got { version, items }),
        _ => None,
    })
}

/// Each item as the text it covers in `text` and its message.
fn words(items: &[Diagnostic], text: &str) -> Vec<(String, String)> {
    (items.iter())
        .map(|item| (text[item.range.clone()].to_owned(), item.message.clone()))
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    (pairs.iter())
        .map(|(word, message)| ((*word).to_owned(), (*message).to_owned()))
        .collect()
}

#[test]
fn decodes_tsserver_diagnostics() {
    // A multibyte character and an emoji (two UTF-16 code units, four
    // bytes) before the ranges, on the same line.
    let text = "let a = 1;\nconst é😀 = broken + unused; old(); hint;\n";
    let bodies = || {
        [
            vec![
                over(text, "😀", "error", 1127, json!({})),
                // No offset: skipped.
                json!({ "start": { "line": 2 }, "end": { "line": 2, "offset": 3 },
                        "text": "malformed", "category": "error" }),
            ],
            vec![
                over(text, "broken", "error", 2304, json!({})),
                over(text, "hint", "warning", 2695, json!({})),
                // Not a diagnostic at all: skipped.
                json!("malformed"),
                // An unknown category: skipped.
                over(text, "a", "fatal", 1, json!({})),
            ],
            vec![
                over(
                    text,
                    "unused",
                    "suggestion",
                    6133,
                    json!({ "reportsUnnecessary": {} }),
                ),
                over(
                    text,
                    "old",
                    "suggestion",
                    6387,
                    json!({ "reportsDeprecated": true }),
                ),
                over(
                    text,
                    "const",
                    "message",
                    80001,
                    json!({ "reportsUnnecessary": false }),
                ),
            ],
        ]
    };
    let expected = [
        ("😀", Severity::Error, "1127", vec![]),
        ("broken", Severity::Error, "2304", vec![]),
        ("hint", Severity::Warning, "2695", vec![]),
        (
            "unused",
            Severity::Hint,
            "6133",
            vec![DiagnosticTag::Unnecessary],
        ),
        (
            "old",
            Severity::Hint,
            "6387",
            vec![DiagnosticTag::Deprecated],
        ),
        ("const", Severity::Information, "80001", vec![]),
    ];
    // Offsets are UTF-16 whatever the session negotiated.
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let mut s = Tsserver::new(Some(encoding.clone()));
        let mut buffer = Buffer::new(text);
        s.fake.set_writes(Writes::Paced);
        s.open(&mut buffer, "a.ts");
        s.step(responses(bodies()), |_| {});
        let got = next(&s.events);
        assert_eq!(got.version, buffer.version());
        let decoded: Vec<_> = (got.items.iter())
            .map(|item| {
                (
                    &text[item.range.clone()],
                    item.severity.unwrap(),
                    item.code.as_deref().unwrap(),
                    item.tags.clone(),
                )
            })
            .collect();
        assert_eq!(decoded, expected, "under {encoding:?}");
        for item in &got.items {
            assert_eq!(item.source.as_deref(), Some("typescript"));
            assert_eq!(
                item.message,
                format!(
                    "{} {}",
                    match item.severity.unwrap() {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                        Severity::Hint => "suggestion",
                        Severity::Information => "message",
                    },
                    item.code.as_deref().unwrap()
                )
            );
        }
    }
}

#[test]
fn pulls_through_tsserver_request() {
    let mut s = Tsserver::new(Some(PositionEncodingKind::UTF16));
    let v0 = "let alpha = 1;\n";
    let mut buffer = Buffer::new(v0);
    let at0 = buffer.version();

    // didOpen, then the three requests, whose answers make one list for
    // version 0.
    s.fake.set_writes(Writes::Paced);
    let uri = s.open(&mut buffer, "a.ts");
    s.step(
        responses([
            vec![over(v0, "let", "error", 1, json!({}))],
            vec![over(v0, "alpha", "error", 2, json!({}))],
            vec![over(v0, "1", "suggestion", 3, json!({}))],
        ]),
        |_| {},
    );
    let got = next(&s.events);
    assert_eq!(got.version, at0);
    assert_eq!(
        words(&got.items, v0),
        pairs(&[
            ("let", "error 1"),
            ("alpha", "error 2"),
            ("1", "suggestion 3")
        ]),
    );
    let sent = executes(&s.fake.records());
    assert_eq!(sent.len(), 3);
    for (params, command) in sent.iter().zip(COMMANDS) {
        assert_eq!(
            *params,
            json!({
                "command": "typescript.tsserverRequest",
                "arguments": [command, { "file": uri }],
            }),
        );
    }

    // A change, and another made once the first request is on the wire:
    // its didChange goes behind all three, which belong to version 1, and
    // the pull it wants waits for their answers.
    let v1 = "// one\nlet alpha = 1;\n";
    let v2 = "// one\nlet alpha = 12;\n";
    s.fake.set_writes(Writes::Paced);
    s.edit(&mut buffer, (0..0, "// one\n"));
    let at1 = buffer.version();
    let one = v1.find('1').unwrap();
    s.step(
        responses([
            vec![],
            vec![over(v1, "alpha", "error", 1, json!({}))],
            vec![],
        ]),
        |n| {
            if n == 1 {
                s.edit(&mut buffer, (one + 1..one + 1, "2"));
            }
        },
    );
    assert_eq!(buffer.text().to_string(), v2);
    let got = next(&s.events);
    assert_eq!(got.version, at1);
    assert_eq!(words(&got.items, v1), pairs(&[("alpha", "error 1")]));

    // Version 2's pull, sent once those answers came: one of its three
    // fails, so it's dropped.
    s.step(
        [
            response(COMMANDS[0], vec![over(v2, "12", "error", 1, json!({}))]),
            Reply::Error {
                code: 1,
                message: "TypeScript Server Error".into(),
            },
            response(COMMANDS[2], vec![]),
        ],
        |_| {},
    );
    s.fake.set_writes(Writes::Open);
    s.barrier();
    s.events.assert_quiet();

    // The server's own pushes carry no version, and are still dropped.
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![lsp_types::Diagnostic::default()],
            version: None,
        });
    s.barrier();
    s.events.assert_quiet();

    // The next change is pulled again, at its version.
    let v3 = "// one\nlet alpha = 123;\n";
    s.fake.set_writes(Writes::Paced);
    s.edit(&mut buffer, (one + 2..one + 2, "3"));
    assert_eq!(buffer.text().to_string(), v3);
    s.step(
        responses([vec![over(v3, "123", "error", 1, json!({}))], vec![], vec![]]),
        |_| {},
    );
    s.fake.set_writes(Writes::Open);
    let got = next(&s.events);
    assert_eq!(got.version, buffer.version());
    assert_eq!(words(&got.items, v3), pairs(&[("123", "error 1")]));

    // Each pull's three went back to back, right behind what they followed.
    let pull = COMMANDS.map(str::to_owned);
    let mut expected = vec!["didOpen".to_owned()];
    for _ in 0..4 {
        if expected.len() > 1 {
            expected.push("didChange".into());
        }
        expected.extend(pull.clone());
    }
    assert_eq!(sequence(&s.fake, &uri), expected);
    assert!(
        s.fake.sync_errors().is_empty(),
        "{:?}",
        s.fake.sync_errors()
    );
}

#[test]
fn builtin_specs() {
    let specs = registry::builtin_specs();
    let spec = |name: &str| specs.iter().find(|spec| spec.name == name).unwrap();
    let tsls = spec("typescript-language-server");
    assert_eq!(tsls.pull_diagnostics, PullDiagnostics::TsserverRequest);
    assert_eq!(tsls.unversioned_diagnostics, UnversionedDiagnostics::Drop);
    for name in ["typescript-go", "pyright", "rust-analyzer"] {
        assert_eq!(
            spec(name).pull_diagnostics,
            PullDiagnostics::Advertised,
            "{name}"
        );
    }
    assert_eq!(PullDiagnostics::default(), PullDiagnostics::Advertised);
    assert_eq!(
        ServerSpec::new("x", "x").pull_diagnostics,
        PullDiagnostics::Advertised
    );
}
