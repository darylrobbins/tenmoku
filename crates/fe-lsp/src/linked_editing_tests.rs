//! Linked editing ranges: replies decoded in bytes of the version in a
//! UTF-8 and a UTF-16 session, a server without the provider, a `null`
//! reply and a timeout, and the word pattern.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{LinkedEditingRange, Request};
use lsp_types::{
    LinkedEditingRangeServerCapabilities, Position, PositionEncodingKind, ServerCapabilities, Url,
};
use serde_json::{Value, json};

use super::LinkedEditing;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{LspEvent, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = "/fake/root";
const WAIT: Duration = Duration::from_secs(10);

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
    utf16: bool,
}

impl Started {
    fn new(encoding: PositionEncodingKind, provider: bool, text: &str) -> Self {
        let utf16 = encoding == PositionEncodingKind::UTF16;
        let mut init = initialize_result(Some(encoding));
        init.capabilities = ServerCapabilities {
            linked_editing_range_provider: provider
                .then_some(LinkedEditingRangeServerCapabilities::Simple(true)),
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
        let (sender, events) = std::sync::mpsc::channel::<LspEvent>();
        let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), move |event| {
            let _ = sender.send(event);
        });
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        fake.wait_initialized();
        // The events aren't read; keep the channel open.
        std::mem::forget(events);

        let mut buffer = Buffer::new(text);
        let path = PathBuf::from(ROOT).join("a.html");
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
                "html",
            )
            .unwrap();
        fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        Self {
            fake,
            clock,
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

    fn ask(&self, offset: usize) -> Result<Option<LinkedEditing>, crate::LspError> {
        answer(
            self.manager
                .linked_editing_ranges(self.buffer.id(), self.buffer.version(), offset),
        )
    }

    fn requests(&self) -> Vec<(Value, Value)> {
        linked_requests(&self.fake.records())
    }
}

/// Waits for `reply`; panics after [`WAIT`] of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = futures::FutureExt::fuse(crate::Clock::sleep(&SystemClock, WAIT));
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in {WAIT:?}"),
        }
    })
}

fn linked_requests(records: &[Record]) -> Vec<(Value, Value)> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { id, method, params } if method == LinkedEditingRange::METHOD => {
                Some((json!(id), params.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn ranges_decode_to_bytes_utf8_and_utf16() {
    // An emoji before the tag: four bytes, two UTF-16 units.
    let text = "🦀<div></div>\n";
    let open = text.find("div").unwrap();
    let close = text.rfind("div").unwrap();
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let s = Started::new(encoding, true, text);
        let at = |start: usize| start..start + 3;
        // Out of order, with the open tag's name twice.
        let reply = json!({ "ranges": [
            s.range(text, at(close)),
            s.range(text, at(open)),
            s.range(text, at(open)),
        ] });
        s.fake.set_reply(
            LinkedEditingRange::METHOD,
            Scripted::now(Reply::Result(reply)),
        );
        let found = s.ask(open + 1).unwrap().unwrap();
        assert_eq!(found.ranges, vec![at(open), at(close)]);
        assert_eq!(found.word_pattern, None);
        let (_, params) = s.requests().pop().unwrap();
        assert_eq!(params["position"], json!(s.position(text, open + 1)));
    }
}

#[test]
fn no_provider_sends_nothing() {
    let s = Started::new(PositionEncodingKind::UTF16, false, "<div></div>\n");
    assert_eq!(s.ask(2), Ok(None));
    assert!(s.requests().is_empty());
}

#[test]
fn null_reply_and_timeout_are_empty() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "<div></div>\n");
    let method = LinkedEditingRange::METHOD;

    s.fake
        .set_reply(method, Scripted::now(Reply::Result(Value::Null)));
    assert_eq!(s.ask(2), Ok(Some(LinkedEditing::default())));

    // Past `lsp.timeouts.hover`'s 3 s: no ranges, and the request is
    // cancelled.
    s.fake.set_reply(
        method,
        Scripted {
            reply: Reply::Result(json!({ "ranges": [] })),
            delay: Duration::from_secs(60),
        },
    );
    let count = s.requests().len();
    let mut late = s
        .manager
        .linked_editing_ranges(s.buffer.id(), s.buffer.version(), 2)
        .boxed();
    let records = s.fake.wait_for("the late request", |records| {
        linked_requests(records).len() > count
    });
    let (id, _) = linked_requests(&records).pop().unwrap();
    s.clock.wait_for_sleep(Duration::from_secs(3));
    s.clock.advance(Duration::from_millis(2999));
    std::thread::sleep(Duration::from_millis(50));
    assert!(late.as_mut().now_or_never().is_none());
    s.clock.advance(Duration::from_millis(1));
    assert_eq!(answer(late), Ok(Some(LinkedEditing::default())));
    s.fake.wait_for("$/cancelRequest", |records| {
        records.iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == id)
        })
    });
}

#[test]
fn word_pattern_is_returned() {
    let text = "<div></div>\n";
    let s = Started::new(PositionEncodingKind::UTF16, true, text);
    s.fake.set_reply(
        LinkedEditingRange::METHOD,
        Scripted::now(Reply::Result(json!({
            "ranges": [s.range(text, 1..4), s.range(text, 7..10)],
            "wordPattern": "[a-zA-Z][-a-zA-Z0-9]*",
        }))),
    );
    let found = s.ask(2).unwrap().unwrap();
    assert_eq!(found.ranges, vec![1..4, 7..10]);
    assert_eq!(found.word_pattern.as_deref(), Some("[a-zA-Z][-a-zA-Z0-9]*"));
}
