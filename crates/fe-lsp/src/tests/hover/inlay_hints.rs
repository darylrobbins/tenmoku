//! `textDocument/inlayHint` and `inlayHint/resolve`: hints decoded in bytes
//! of the version in a UTF-8 and a UTF-16 session, a server without the
//! provider, resolve and its timeout, and the capabilities advertised.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{InlayHintRequest, InlayHintResolveRequest, Request};
use lsp_types::{
    InlayHintOptions, InlayHintServerCapabilities, NumberOrString, OneOf, PositionEncodingKind,
    ServerCapabilities,
};
use serde_json::{Value, json};

use super::super::{Started, answer, manager};
use super::position;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{InlayHint, InlayHintKind, ManualClock, ServerSpec};

struct Hints {
    s: Started,
    clock: Arc<ManualClock>,
    buffer: Buffer,
    utf16: bool,
}

fn start(encoding: PositionEncodingKind, provider: Option<bool>, text: &str) -> Hints {
    let utf16 = encoding == PositionEncodingKind::UTF16;
    let mut init = initialize_result(Some(encoding));
    init.capabilities = ServerCapabilities {
        inlay_hint_provider: provider.map(|resolve| {
            OneOf::Right(InlayHintServerCapabilities::Options(InlayHintOptions {
                resolve_provider: Some(resolve),
                ..Default::default()
            }))
        }),
        ..init.capabilities
    };
    let fake = FakeServer::with_clock(
        Script {
            initialize: InitializeReply::Result(Box::new(init)),
            ..Script::default()
        },
        Arc::new(ManualClock::new()),
    );
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            Path::new(super::super::ROOT),
        )
        .unwrap();
    events.status(server);
    fake.wait_initialized();
    let mut s = Started {
        fake,
        manager,
        server,
        events,
    };
    let mut buffer = Buffer::new(text);
    s.open_synced(&mut buffer, "a.rs");
    Hints {
        s,
        clock,
        buffer,
        utf16,
    }
}

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

// An emoji before the hints: four bytes, two UTF-16 units.
const TEXT: &str = "let 🦀 = f(1);\nlet y = 2;\n";

#[test]
fn hints_decode_in_bytes_of_the_version() {
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let h = start(encoding, Some(false), TEXT);
        let at = |offset: usize| position(TEXT, offset, h.utf16);
        let crab_end = TEXT.find(" =").unwrap();
        let arg = TEXT.find('1').unwrap();
        let later = TEXT.find("\nlet y").unwrap() + 1;
        let y_end = later + "let y".len();
        let range = 0..later;
        h.s.fake.set_reply(
            InlayHintRequest::METHOD,
            Scripted::now(Reply::Result(json!([
                {
                    "position": at(arg),
                    "label": [{ "value": "na" }, { "value": "me:" }],
                    "kind": 2,
                    "paddingRight": true,
                },
                // Outside the asked range.
                { "position": at(y_end), "label": ": i32", "kind": 1 },
                { "position": at(crab_end), "label": ": u8", "kind": 1, "paddingLeft": true },
            ]))),
        );
        let found = answer(h.s.manager.inlay_hints(
            h.buffer.id(),
            h.buffer.version(),
            range.clone(),
        ))
        .unwrap()
        .unwrap();
        let summary: Vec<_> = (found.iter())
            .map(|hint| {
                (
                    hint.offset,
                    hint.label.as_str(),
                    hint.kind,
                    hint.padding_left,
                    hint.padding_right,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (crab_end, ": u8", Some(InlayHintKind::Type), true, false),
                (arg, "name:", Some(InlayHintKind::Parameter), false, true),
            ]
        );
        let (_, params) = requests(&h.s.fake.records(), InlayHintRequest::METHOD)
            .pop()
            .unwrap();
        assert_eq!(
            params["range"],
            json!({ "start": at(range.start), "end": at(range.end) })
        );
    }
}

#[test]
fn no_provider_sends_nothing() {
    let h = start(PositionEncodingKind::UTF16, None, TEXT);
    let found = answer(
        h.s.manager
            .inlay_hints(h.buffer.id(), h.buffer.version(), 0..5),
    );
    assert_eq!(found, Ok(None));
    assert!(requests(&h.s.fake.records(), InlayHintRequest::METHOD).is_empty());
}

#[test]
fn resolve_fills_tooltip_and_edits() {
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let h = start(encoding, Some(true), TEXT);
        let at = |offset: usize| position(TEXT, offset, h.utf16);
        let crab_end = TEXT.find(" =").unwrap();
        let hint_json = json!({
            "position": at(crab_end),
            "label": ": u8",
            "kind": 1,
            "data": { "token": 7 },
        });
        h.s.fake.set_reply(
            InlayHintRequest::METHOD,
            Scripted::now(Reply::Result(json!([hint_json]))),
        );
        let mut found = answer(h.s.manager.inlay_hints(
            h.buffer.id(),
            h.buffer.version(),
            0..TEXT.len(),
        ))
        .unwrap()
        .unwrap();
        let hint: InlayHint = found.pop().unwrap();
        assert_eq!((hint.tooltip.clone(), hint.edits.clone()), (None, vec![]));

        h.s.fake.set_reply(
            InlayHintResolveRequest::METHOD,
            Scripted::now(Reply::Result(json!({
                "position": at(crab_end),
                "label": ": u8",
                "data": { "token": 7 },
                "tooltip": { "kind": "markdown", "value": "`u8`: an integer" },
                "textEdits": [{
                    "range": { "start": at(crab_end), "end": at(crab_end) },
                    "newText": ": u8",
                }],
            }))),
        );
        let resolved = answer(h.s.manager.resolve_inlay_hint(hint.clone())).unwrap();
        assert_eq!(resolved.tooltip.as_deref(), Some("`u8`: an integer"));
        assert_eq!(
            resolved.edits,
            vec![(crab_end..crab_end, ": u8".to_owned())]
        );
        assert_eq!(resolved.offset, crab_end);
        // The hint's own data went back.
        let (_, params) = requests(&h.s.fake.records(), InlayHintResolveRequest::METHOD)
            .pop()
            .unwrap();
        assert_eq!(params["data"], json!({ "token": 7 }));

        // Past `lsp.timeouts.resolve`'s 2 s: the hint as it was, and the
        // request is cancelled.
        h.s.fake.set_reply(
            InlayHintResolveRequest::METHOD,
            Scripted {
                reply: Reply::Result(json!({
                    "position": at(crab_end),
                    "label": ": u8",
                    "tooltip": "late",
                })),
                delay: Duration::from_secs(60),
            },
        );
        let count = requests(&h.s.fake.records(), InlayHintResolveRequest::METHOD).len();
        let mut late = h.s.manager.resolve_inlay_hint(hint.clone()).boxed();
        let records = h.s.fake.wait_for("the late resolve", |records| {
            requests(records, InlayHintResolveRequest::METHOD).len() > count
        });
        let (id, _) = requests(&records, InlayHintResolveRequest::METHOD)
            .pop()
            .unwrap();
        h.clock.wait_for_sleep(Duration::from_secs(2));
        h.clock.advance(Duration::from_millis(1999));
        std::thread::sleep(Duration::from_millis(50));
        assert!(late.as_mut().now_or_never().is_none());
        h.clock.advance(Duration::from_millis(1));
        assert_eq!(answer(late), Ok(hint));
        h.s.fake.wait_for("$/cancelRequest", |records| {
            records.iter().any(|record| {
                matches!(record, Record::Notification { method, params }
                    if method == "$/cancelRequest" && params["id"] == json!(id))
            })
        });
    }
}

#[test]
fn inlay_hint_capabilities_are_advertised() {
    let h = start(PositionEncodingKind::UTF16, Some(false), TEXT);
    let init = (h.s.fake.records().into_iter())
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        init["capabilities"]["textDocument"]["inlayHint"],
        json!({
            "dynamicRegistration": false,
            "resolveSupport": { "properties": ["tooltip", "textEdits", "label.tooltip"] },
        })
    );
    assert_eq!(
        init["capabilities"]["workspace"]["inlayHint"],
        json!({ "refreshSupport": true })
    );
}
