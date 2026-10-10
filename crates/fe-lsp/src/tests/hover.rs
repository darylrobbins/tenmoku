//! `textDocument/hover` (§7, "Hover"): each shape of reply as text, its
//! range in bytes of the version asked about, and the 3 s timeout as no
//! hover.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{HoverRequest, Request};
use lsp_types::{NumberOrString, Position, PositionEncodingKind};
use serde_json::{Value, json};

use super::{Started, answer, manager};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{Hover, ManualClock, ServerSpec, ServerStatus};

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

/// The hover requests the fake got: each one's id and params.
fn hovers(records: &[Record]) -> Vec<(NumberOrString, Value)> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { id, method, params } if method == HoverRequest::METHOD => {
                Some((id.clone(), params.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn hover_replies_convert() {
    for (encoding, utf16) in [
        (PositionEncodingKind::UTF8, false),
        (PositionEncodingKind::UTF16, true),
    ] {
        // The fake's delays and the manager's timeouts each on a clock of
        // their own.
        let fake_clock = Arc::new(ManualClock::new());
        let fake = FakeServer::with_clock(
            Script {
                initialize: InitializeReply::Result(Box::new(initialize_result(Some(
                    encoding.clone(),
                )))),
                replies: HashMap::new(),
                ..Script::default()
            },
            fake_clock.clone(),
        );
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
        let mut s = Started {
            fake,
            manager,
            server,
            events,
        };

        // The client's capabilities.
        let init = (s.fake.records().into_iter())
            .find_map(|record| match record {
                Record::Request { method, params, .. } if method == "initialize" => Some(params),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            init["capabilities"]["textDocument"]["hover"]["contentFormat"],
            json!(["markdown", "plaintext"])
        );

        // An emoji earlier on the line: two UTF-16 units, four bytes.
        let v0 = "let 🦀 = alpha.beta();\n";
        let beta = v0.find("beta").unwrap();
        let mut buffer = Buffer::new(v0);
        let uri = s.open_synced(&mut buffer, "a.rs");
        let range = json!({
            "start": position(v0, beta, utf16),
            "end": position(v0, beta + 4, utf16),
        });
        let ask = |s: &Started, buffer: &Buffer, reply: Value| {
            s.fake
                .set_reply(HoverRequest::METHOD, Scripted::now(Reply::Result(reply)));
            let hover = answer(s.manager.hover(buffer.id(), buffer.version(), beta + 1));
            // Asked about where the offset is, in the session's encoding.
            let (_, params) = hovers(&s.fake.records()).pop().unwrap();
            assert_eq!(params["textDocument"]["uri"], json!(uri));
            assert_eq!(params["position"], position(v0, beta + 1, utf16));
            hover.unwrap()
        };
        let code = "```rust\nfn beta()\n```";

        // Markdown, as it is, with its range in bytes.
        let markdown = format!("{code}\nCalls *beta*.");
        assert_eq!(
            ask(
                &s,
                &buffer,
                json!({
                    "contents": { "kind": "markdown", "value": markdown },
                    "range": range,
                })
            ),
            Some(Hover::new(markdown.clone(), Some(beta..beta + 4)))
        );
        // Plain text, as it is; no range.
        assert_eq!(
            ask(
                &s,
                &buffer,
                json!({ "contents": { "kind": "plaintext", "value": "fn beta()" } })
            ),
            Some(Hover::new("fn beta()", None))
        );
        // A marked string in a language: a fenced block.
        assert_eq!(
            ask(
                &s,
                &buffer,
                json!({
                    "contents": { "language": "rust", "value": "fn beta()" },
                    "range": range,
                })
            ),
            Some(Hover::new(code, Some(beta..beta + 4)))
        );
        // An array of them, joined a blank line apart; empty ones dropped.
        assert_eq!(
            ask(
                &s,
                &buffer,
                json!({
                    "contents": [
                        { "language": "rust", "value": "fn beta()" },
                        "",
                        "Calls *beta*.",
                    ],
                })
            ),
            Some(Hover::new(format!("{code}\n\nCalls *beta*."), None))
        );
        // Nothing to say.
        assert_eq!(ask(&s, &buffer, Value::Null), None);
        assert_eq!(ask(&s, &buffer, json!({ "contents": [] })), None);

        // The range is in the version asked about, though the buffer has
        // moved on (a line break before the emoji line) by the reply.
        s.fake.set_reply(
            HoverRequest::METHOD,
            Scripted {
                reply: Reply::Result(json!({
                    "contents": { "kind": "markdown", "value": "beta" },
                    "range": range,
                })),
                delay: Duration::from_secs(1),
            },
        );
        // `hover` sends at once, so count before asking or the request may
        // already be counted.
        let count = hovers(&s.fake.records()).len();
        let asked = s.manager.hover(buffer.id(), buffer.version(), beta);
        s.fake
            .wait_for("the hover", |records| hovers(records).len() > count);
        s.edit(&mut buffer, &uri, (0..0, "\n"), 1);
        fake_clock.wait_for_sleep(Duration::from_secs(1));
        fake_clock.advance(Duration::from_secs(1));
        assert_eq!(
            answer(asked).unwrap(),
            Some(Hover::new("beta", Some(beta..beta + 4)))
        );

        // A server that answers after `lsp.timeouts.hover`'s 3 s: no
        // hover, and the request is cancelled.
        s.fake.set_reply(
            HoverRequest::METHOD,
            Scripted {
                reply: Reply::Result(json!({ "contents": "late" })),
                delay: Duration::from_secs(60),
            },
        );
        let count = hovers(&s.fake.records()).len();
        let mut late = s
            .manager
            .hover(buffer.id(), buffer.version(), beta + 1)
            .boxed();
        let records = s
            .fake
            .wait_for("the late hover", |records| hovers(records).len() > count);
        let (id, _) = hovers(&records).pop().unwrap();
        clock.wait_for_sleep(Duration::from_secs(3));
        clock.advance(Duration::from_millis(2999));
        std::thread::sleep(Duration::from_millis(50));
        assert!(late.as_mut().now_or_never().is_none());
        clock.advance(Duration::from_millis(1));
        assert_eq!(answer(late), Ok(None));
        s.fake.wait_for("$/cancelRequest", |records| {
            records.iter().any(|record| {
                matches!(record, Record::Notification { method, params }
                    if method == "$/cancelRequest" && params["id"] == json!(id))
            })
        });
    }
}
