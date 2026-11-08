//! Folding ranges: replies decoded to rows, a server without the provider,
//! a `null` reply and a timeout, and the client's declared capability.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{FoldingRangeRequest, Request};
use lsp_types::{FoldingRangeProviderCapability, PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::{FoldKind, FoldingRange};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{Clock, LspEvent, LspManager, ManualClock, ServerSpec, SystemClock};

/// Waits for `reply`; panics after 10 s of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = SystemClock.sleep(Duration::from_secs(10)).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in 10 s"),
        }
    })
}

const ROOT: &str = crate::fake::FAKE_ROOT;

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
    uri: Url,
}

impl Started {
    fn new(encoding: PositionEncodingKind, provider: bool, text: &str) -> Self {
        Self::start(encoding, provider, text, Duration::ZERO)
    }

    /// As `new`, but `initialize` is answered `delay` after the fake's
    /// clock starts, and `Started` doesn't wait for that.
    fn start(encoding: PositionEncodingKind, provider: bool, text: &str, delay: Duration) -> Self {
        let mut init = initialize_result(Some(encoding));
        init.capabilities = ServerCapabilities {
            folding_range_provider: provider
                .then_some(FoldingRangeProviderCapability::Simple(true)),
            ..init.capabilities
        };
        let fake = FakeServer::with_clock(
            Script {
                initialize: InitializeReply::Result(Box::new(init)),
                initialize_delay: delay,
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
            manager,
            buffer,
            uri,
        }
    }

    fn requests(&self) -> Vec<(Value, Value)> {
        fold_requests(&self.fake.records())
    }
}

fn fold_requests(records: &[Record]) -> Vec<(Value, Value)> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { id, method, params } if method == FoldingRangeRequest::METHOD => {
                Some((json!(id), params.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn ranges_decode_to_rows() {
    // Columns are ignored: only whole lines fold.
    let reply = json!([
        { "startLine": 4, "endLine": 9 },
        { "startLine": 0, "startCharacter": 7, "endLine": 2, "endCharacter": 1 },
    ]);
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let s = Started::new(encoding, true, "fn a() {\n}\n");
        s.fake.set_reply(
            FoldingRangeRequest::METHOD,
            Scripted::now(Reply::Result(reply.clone())),
        );
        let found = answer(s.manager.folding_ranges(s.buffer.id(), s.buffer.version()));
        assert_eq!(
            found,
            Ok(Some(vec![
                FoldingRange::new(4, 9, None),
                FoldingRange::new(0, 2, None),
            ]))
        );
        let (_, params) = s.requests().pop().unwrap();
        assert_eq!(params["textDocument"]["uri"], json!(s.uri));
    }
}

#[test]
fn no_provider_sends_nothing() {
    let s = Started::new(PositionEncodingKind::UTF16, false, "alpha\n");
    let found = answer(s.manager.folding_ranges(s.buffer.id(), s.buffer.version()));
    assert_eq!(found, Ok(None));
    assert!(s.requests().is_empty());
}

#[test]
fn null_reply_and_timeout_are_empty() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "alpha\n");
    let method = FoldingRangeRequest::METHOD;

    s.fake
        .set_reply(method, Scripted::now(Reply::Result(Value::Null)));
    let found = answer(s.manager.folding_ranges(s.buffer.id(), s.buffer.version()));
    assert_eq!(found, Ok(Some(Vec::new())));

    // Past `lsp.timeouts.hover`'s 3 s: no folds, and the request is
    // cancelled.
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
        .folding_ranges(s.buffer.id(), s.buffer.version())
        .boxed();
    let records = s.fake.wait_for("the late request", |records| {
        fold_requests(records).len() > count
    });
    let (id, _) = fold_requests(&records).pop().unwrap();
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
fn inverted_and_single_row_ranges_are_dropped() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "alpha\n");
    s.fake.set_reply(
        FoldingRangeRequest::METHOD,
        Scripted::now(Reply::Result(json!([
            { "startLine": 3, "endLine": 3 },
            { "startLine": 8, "endLine": 5 },
            { "startLine": 1, "endLine": 2 },
            { "startLine": 6, "endLine": 7 },
        ]))),
    );
    let found = answer(s.manager.folding_ranges(s.buffer.id(), s.buffer.version()));
    assert_eq!(
        found,
        Ok(Some(vec![
            FoldingRange::new(1, 2, None),
            FoldingRange::new(6, 7, None),
        ]))
    );
}

#[test]
fn kinds_decode() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "alpha\n");
    s.fake.set_reply(
        FoldingRangeRequest::METHOD,
        Scripted::now(Reply::Result(json!([
            { "startLine": 0, "endLine": 1, "kind": "comment" },
            { "startLine": 2, "endLine": 3, "kind": "imports" },
            { "startLine": 4, "endLine": 5, "kind": "region" },
            { "startLine": 8, "endLine": 9 },
        ]))),
    );
    let found = answer(s.manager.folding_ranges(s.buffer.id(), s.buffer.version()));
    let kinds: Vec<_> = found.unwrap().unwrap().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            Some(FoldKind::Comment),
            Some(FoldKind::Imports),
            Some(FoldKind::Region),
            None
        ]
    );
}

#[test]
fn the_client_asks_for_whole_lines() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "alpha\n");
    let records = s.fake.records();
    let init = records
        .iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .expect("initialize");
    let folding = &init["capabilities"]["textDocument"]["foldingRange"];
    assert_eq!(folding["lineFoldingOnly"], json!(true));
    assert_eq!(folding["rangeLimit"], json!(5000));
}
