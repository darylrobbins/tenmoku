//! The caller count for a hover card's footer: the call hierarchy prepared
//! at the symbol, its first item's incoming calls counted, at most 99.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::{CallHierarchyServerCapability, PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::hierarchy::{CALLER_COUNT_CAP, HierarchyItem};
use crate::{LspEvent, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = crate::fake::FAKE_ROOT;
const PREPARE_CALLS: &str = "textDocument/prepareCallHierarchy";
const INCOMING: &str = "callHierarchy/incomingCalls";

fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = crate::Clock::sleep(&SystemClock, Duration::from_secs(10)).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in 10 s"),
        }
    })
}

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
}

impl Started {
    fn new(encoding: PositionEncodingKind, providers: bool, text: &str) -> Self {
        let mut init = initialize_result(Some(encoding));
        init.capabilities = ServerCapabilities {
            call_hierarchy_provider: providers
                .then_some(CallHierarchyServerCapability::Simple(true)),
            ..init.capabilities
        };
        let fake_clock = Arc::new(ManualClock::new());
        let fake = FakeServer::with_clock(
            Script {
                initialize: InitializeReply::Result(Box::new(init)),
                ..Script::default()
            },
            fake_clock,
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
        fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        Self {
            fake,
            clock,
            manager,
            buffer,
        }
    }

    fn reply(&self, method: &str, result: Value) {
        self.fake
            .set_reply(method, Scripted::now(Reply::Result(result)));
    }

    fn prepare_calls(&self) -> Option<Vec<HierarchyItem>> {
        answer(
            self.manager
                .prepare_call_hierarchy(self.buffer.id(), self.buffer.version(), 1),
        )
        .unwrap()
    }

    /// The caller count, asked the way a host does: prepare, then count.
    fn callers(&self) -> Option<usize> {
        let item = self.prepare_calls()?.into_iter().next()?;
        Some(answer(self.manager.caller_count(self.buffer.id(), &item)).unwrap())
    }

    fn requests(&self, method: &str) -> Vec<Value> {
        (self.fake.records().iter())
            .filter_map(|record| match record {
                Record::Request {
                    method: m, params, ..
                } if m == method => Some(params.clone()),
                _ => None,
            })
            .collect()
    }
}

fn uri(name: &str) -> String {
    Url::from_file_path(PathBuf::from(ROOT).join(name))
        .unwrap()
        .to_string()
}

fn range(line: u32, start: u32, end: u32) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

fn item(name: &str, line: u32) -> Value {
    json!({
        "name": name,
        "kind": 12,
        "uri": uri("a.rs"),
        "range": range(line, 0, 12),
        "selectionRange": range(line, 3, 8),
    })
}

fn callers_from(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(|i| json!({ "from": item(&format!("caller{i}"), i as u32), "fromRanges": [range(i as u32, 0, 1)] }))
            .collect(),
    )
}

#[test]
fn counts_incoming_call_items() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 0)]));
    s.reply(INCOMING, callers_from(3));
    assert_eq!(s.callers(), Some(3));
    // The item the server prepared went back to it.
    assert_eq!(s.requests(INCOMING)[0]["item"]["name"], "alpha");
    s.reply(INCOMING, Value::Null);
    assert_eq!(s.callers(), Some(0));
}

#[test]
fn caps_at_99() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 0)]));
    s.reply(INCOMING, callers_from(98));
    assert_eq!(s.callers(), Some(98));
    s.reply(INCOMING, callers_from(250));
    assert_eq!(s.callers(), Some(CALLER_COUNT_CAP));
    assert_eq!(CALLER_COUNT_CAP, 99);
}

#[test]
fn a_server_without_call_hierarchy_gives_none() {
    let s = Started::new(PositionEncodingKind::UTF16, false, "fn alpha() {}\n");
    assert_eq!(s.callers(), None);
    assert!(s.requests(PREPARE_CALLS).is_empty());
    assert!(s.requests(INCOMING).is_empty());
    // And a server with it but no symbol there.
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, Value::Null);
    assert_eq!(s.callers(), None);
    assert!(s.requests(INCOMING).is_empty());
}

#[test]
fn a_timeout_is_not_a_count() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 0)]));
    let target = s.prepare_calls().unwrap().remove(0);
    s.fake.set_reply(
        INCOMING,
        Scripted {
            reply: Reply::Result(json!([])),
            delay: Duration::from_secs(600),
        },
    );
    let mut late = s.manager.caller_count(s.buffer.id(), &target).boxed();
    s.fake.wait_for("the late request", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { method, .. } if method == INCOMING))
    });
    // Past the references timeout of 30 s: unknown, where an empty answer
    // is zero.
    s.clock.wait_for_sleep(Duration::from_secs(30));
    s.clock.advance(Duration::from_secs(30));
    assert_eq!(answer(&mut late), Err(crate::LspError::Timeout));
    s.reply(INCOMING, json!([]));
    assert_eq!(s.callers(), Some(0));
}
