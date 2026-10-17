//! Document highlights: replies decoded in bytes of the version in a UTF-8
//! and a UTF-16 session, a server without the provider, and a `null` reply
//! and a timeout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{DocumentHighlightRequest, Request};
use lsp_types::{OneOf, Position, PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::answer;
use crate::document_highlight::{DocumentHighlight, HighlightKind};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{LspEvent, LspManager, ManualClock, ServerSpec};

const ROOT: &str = "/fake/root";

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    fake_clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
    utf16: bool,
}

impl Started {
    fn new(encoding: PositionEncodingKind, provider: bool, text: &str) -> Self {
        Self::start(encoding, provider, text, Duration::ZERO)
    }

    /// As `new`, but `initialize` is answered `delay` after the fake's
    /// clock starts, and `Started` doesn't wait for that.
    fn start(encoding: PositionEncodingKind, provider: bool, text: &str, delay: Duration) -> Self {
        let utf16 = encoding == PositionEncodingKind::UTF16;
        let mut init = initialize_result(Some(encoding));
        init.capabilities = ServerCapabilities {
            document_highlight_provider: provider.then_some(OneOf::Left(true)),
            ..init.capabilities
        };
        let fake_clock = Arc::new(ManualClock::new());
        let fake = FakeServer::with_clock(
            Script {
                initialize: InitializeReply::Result(Box::new(init)),
                initialize_delay: delay,
                ..Script::default()
            },
            fake_clock.clone(),
        );
        let clock = Arc::new(ManualClock::new());
        let (sender, events) = std::sync::mpsc::channel::<LspEvent>();
        let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), move |event| {
            let _ = sender.send(event);
        });
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        if delay.is_zero() {
            fake.wait_initialized();
        }
        // The events aren't read; keep the channel open.
        std::mem::forget(events);

        let mut buffer = Buffer::new(text);
        let path = PathBuf::from(ROOT).join("a.rs");
        let uri = Url::from_file_path(&path).unwrap();
        let subscription = buffer.subscribe();
        manager
            .open_on(
                server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                "text",
            )
            .unwrap();
        if delay.is_zero() {
            fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        }
        Self {
            fake,
            clock,
            fake_clock,
            manager,
            buffer,
            utf16,
        }
    }

    /// The LSP position of byte `offset` of `text`, counted as the protocol
    /// says, not with fe-lsp's converter.
    fn position(&self, text: &str, offset: usize) -> Position {
        let before = &text[..offset];
        let line = before.matches('\n').count();
        let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
        let column = if self.utf16 {
            column.encode_utf16().count()
        } else {
            column.len()
        };
        Position::new(line as u32, column as u32)
    }

    fn range(&self, text: &str, range: std::ops::Range<usize>) -> Value {
        json!({
            "start": self.position(text, range.start),
            "end": self.position(text, range.end),
        })
    }

    fn requests(&self) -> Vec<(Value, Value)> {
        highlight_requests(&self.fake.records())
    }
}

fn highlight_requests(records: &[Record]) -> Vec<(Value, Value)> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { id, method, params }
                if method == DocumentHighlightRequest::METHOD =>
            {
                Some((json!(id), params.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn highlights_decode_in_bytes_of_the_version() {
    // An emoji before the symbol: four bytes, two UTF-16 units.
    let text = "let 🦀 = alpha;\nalpha = alpha + 1;\n";
    let first = text.find("alpha").unwrap();
    let second = first + text[first..].find("\nalpha").unwrap() + 1;
    let third = second + "alpha = ".len();
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let s = Started::new(encoding, true, text);
        let at = |start: usize| start..start + 5;
        // Out of order, with kinds 3, 1, 2 and none.
        let reply = json!([
            { "range": s.range(text, at(third)), "kind": 2 },
            { "range": s.range(text, at(second)), "kind": 3 },
            { "range": s.range(text, at(first)), "kind": 1 },
            { "range": s.range(text, 0..3) },
            // The same use twice, with another kind of it between.
            { "range": s.range(text, at(third)), "kind": 3 },
            { "range": s.range(text, at(third)), "kind": 2 },
        ]);
        s.fake.set_reply(
            DocumentHighlightRequest::METHOD,
            Scripted::now(Reply::Result(reply)),
        );
        let found = answer(s.manager.document_highlights(
            s.buffer.id(),
            s.buffer.version(),
            first + 1,
        ))
        .unwrap();
        assert_eq!(
            found,
            Some(vec![
                DocumentHighlight::new(0..3, HighlightKind::Text),
                DocumentHighlight::new(at(first), HighlightKind::Text),
                DocumentHighlight::new(at(second), HighlightKind::Write),
                DocumentHighlight::new(at(third), HighlightKind::Read),
                DocumentHighlight::new(at(third), HighlightKind::Write),
            ])
        );
        let (_, params) = s.requests().pop().unwrap();
        assert_eq!(params["position"], json!(s.position(text, first + 1)));
    }
}

#[test]
fn no_provider_sends_nothing() {
    let text = "alpha\n";
    let s = Started::new(PositionEncodingKind::UTF16, false, text);
    let found = answer(
        s.manager
            .document_highlights(s.buffer.id(), s.buffer.version(), 1),
    );
    assert_eq!(found, Ok(None));
    assert!(s.requests().is_empty());
}

#[test]
fn null_reply_and_timeout_are_nothing_to_highlight() {
    let text = "alpha\n";
    let s = Started::new(PositionEncodingKind::UTF16, true, text);
    let method = DocumentHighlightRequest::METHOD;

    s.fake
        .set_reply(method, Scripted::now(Reply::Result(Value::Null)));
    let found = answer(
        s.manager
            .document_highlights(s.buffer.id(), s.buffer.version(), 1),
    );
    assert_eq!(found, Ok(Some(Vec::new())));

    // Past `lsp.timeouts.hover`'s 3 s: nothing to highlight, and the
    // request is cancelled.
    s.fake.set_reply(
        method,
        Scripted {
            reply: Reply::Result(json!([])),
            delay: Duration::from_secs(60),
        },
    );
    let count = s.requests().len();
    let mut late = s
        .manager
        .document_highlights(s.buffer.id(), s.buffer.version(), 1)
        .boxed();
    let records = s.fake.wait_for("the late request", |records| {
        highlight_requests(records).len() > count
    });
    let (id, _) = highlight_requests(&records).pop().unwrap();
    s.clock.wait_for_sleep(Duration::from_secs(3));
    s.clock.advance(Duration::from_millis(2999));
    std::thread::sleep(Duration::from_millis(50));
    assert!(late.as_mut().now_or_never().is_none());
    s.clock.advance(Duration::from_millis(1));
    assert_eq!(answer(late), Ok(Some(Vec::new())));
    s.fake.wait_for("$/cancelRequest", |records| {
        records.iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == id)
        })
    });
}

#[test]
fn no_provider_sends_nothing_while_initializing() {
    let text = "alpha\n";
    for (provider, found) in [(false, None), (true, Some(vec![]))] {
        let s = Started::start(
            PositionEncodingKind::UTF16,
            provider,
            text,
            Duration::from_secs(1),
        );
        s.fake.set_reply(
            DocumentHighlightRequest::METHOD,
            Scripted::now(Reply::Result(json!([]))),
        );
        // Asked before the server has answered `initialize`.
        let (id, version) = (s.buffer.id(), s.buffer.version());
        let asked = s.manager.document_highlights(id, version, 1);
        let waiting = std::thread::spawn(move || answer(asked));
        s.clock.wait_for_sleep(Duration::from_millis(20));
        assert!(s.requests().is_empty());
        s.fake_clock.wait_for_sleep(Duration::from_secs(1));
        s.fake_clock.advance(Duration::from_secs(1));
        s.fake.wait_initialized();
        s.clock.advance(Duration::from_millis(20));
        assert_eq!(waiting.join().unwrap(), Ok(found));
        // A server with the provider is asked once, and one without never.
        assert_eq!(s.requests().len(), usize::from(provider));
    }
}
