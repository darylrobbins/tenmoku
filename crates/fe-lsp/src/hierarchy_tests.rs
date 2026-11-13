//! Call and type hierarchy: items decoded, follow-ups carrying the raw
//! item, a server without the provider, non-file uris, `null` replies,
//! timeouts and the cap.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use fe_nav::OutlineKind;
use futures::FutureExt;
use lsp_types::{CallHierarchyServerCapability, PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use crate::LspError;
use crate::encoding::PositionEncoding;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::hierarchy::{HierarchyItem, MAX_EDGES};
use crate::{LspEvent, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = crate::fake::FAKE_ROOT;
const PREPARE_CALLS: &str = "textDocument/prepareCallHierarchy";
const PREPARE_TYPES: &str = "textDocument/prepareTypeHierarchy";
const INCOMING: &str = "callHierarchy/incomingCalls";
const OUTGOING: &str = "callHierarchy/outgoingCalls";
const SUPERTYPES: &str = "typeHierarchy/supertypes";
const SUBTYPES: &str = "typeHierarchy/subtypes";

/// Waits for `reply`; panics after 10 s of real time.
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
            experimental: providers.then(|| json!({ "typeHierarchyProvider": true })),
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

fn item(name: &str, kind: u32, file: &str, line: u32) -> Value {
    json!({
        "name": name,
        "kind": kind,
        "detail": "fn()",
        "uri": uri(file),
        "range": range(line, 0, 12),
        "selectionRange": range(line, 3, 8),
        "data": { "token": format!("{name}-data") },
    })
}

#[test]
fn prepare_call_hierarchy_decodes_items() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    let mut deprecated = item("beta", 6, "b.rs", 4);
    deprecated["tags"] = json!([1]);
    s.reply(
        PREPARE_CALLS,
        json!([item("alpha", 12, "a.rs", 0), deprecated]),
    );
    let found = s.prepare_calls().unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name, "alpha");
    assert_eq!(found[0].detail.as_deref(), Some("fn()"));
    assert_eq!(found[0].kind, OutlineKind::Function);
    assert_eq!(found[0].path, PathBuf::from(ROOT).join("a.rs"));
    assert_eq!(
        found[0].range,
        serde_json::from_value(range(0, 0, 12)).unwrap()
    );
    assert_eq!(
        found[0].selection_range,
        serde_json::from_value(range(0, 3, 8)).unwrap()
    );
    assert_eq!(found[0].encoding, PositionEncoding::Utf16);
    assert!(!found[0].deprecated);
    assert_eq!(found[0].raw["data"], json!({ "token": "alpha-data" }));
    assert_eq!(found[1].kind, OutlineKind::Method);
    assert!(found[1].deprecated);
    let sent = &s.requests(PREPARE_CALLS)[0];
    assert_eq!(sent["position"], json!({ "line": 0, "character": 1 }));
}

#[test]
fn incoming_calls_carry_the_callers_call_sites() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 12, "a.rs", 0)]));
    let target = s.prepare_calls().unwrap().remove(0);
    s.reply(
        INCOMING,
        json!([{ "from": item("caller", 12, "b.rs", 7), "fromRanges": [range(8, 2, 7), range(9, 4, 9)] }]),
    );
    let edges = answer(s.manager.incoming_calls(s.buffer.id(), &target)).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].item.name, "caller");
    assert_eq!(edges[0].item.path, PathBuf::from(ROOT).join("b.rs"));
    assert_eq!(
        edges[0].from_ranges,
        serde_json::from_value::<Vec<lsp_types::Range>>(json!([range(8, 2, 7), range(9, 4, 9)]))
            .unwrap()
    );
}

#[test]
fn outgoing_calls_carry_call_sites_in_the_item() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 12, "a.rs", 0)]));
    let source = s.prepare_calls().unwrap().remove(0);
    s.reply(
        OUTGOING,
        json!([{ "to": item("callee", 12, "c.rs", 2), "fromRanges": [range(0, 10, 16)] }]),
    );
    let edges = answer(s.manager.outgoing_calls(s.buffer.id(), &source)).unwrap();
    assert_eq!(edges[0].item.name, "callee");
    assert_eq!(edges[0].item.path, PathBuf::from(ROOT).join("c.rs"));
    assert_eq!(edges[0].from_ranges.len(), 1);
    assert_eq!(edges[0].from_ranges[0].start.character, 10);
}

#[test]
fn raw_item_round_trips_to_the_server() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    let sent = item("alpha", 12, "a.rs", 0);
    s.reply(PREPARE_CALLS, json!([sent.clone()]));
    let target = s.prepare_calls().unwrap().remove(0);
    assert_eq!(target.raw, sent);
    s.reply(INCOMING, Value::Null);
    s.reply(OUTGOING, Value::Null);
    answer(s.manager.incoming_calls(s.buffer.id(), &target)).unwrap();
    answer(s.manager.outgoing_calls(s.buffer.id(), &target)).unwrap();
    assert_eq!(s.requests(INCOMING)[0]["item"], sent);
    assert_eq!(s.requests(OUTGOING)[0]["item"], sent);
}

#[test]
fn no_provider_sends_nothing() {
    let s = Started::new(PositionEncodingKind::UTF16, false, "fn alpha() {}\n");
    assert_eq!(s.prepare_calls(), None);
    let types = answer(
        s.manager
            .prepare_type_hierarchy(s.buffer.id(), s.buffer.version(), 1),
    );
    assert_eq!(types, Ok(None));
    assert!(s.requests(PREPARE_CALLS).is_empty());
    assert!(s.requests(PREPARE_TYPES).is_empty());
}

#[test]
fn non_file_uris_are_dropped() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    let mut remote = item("remote", 12, "a.rs", 0);
    remote["uri"] = json!("https://example.com/a.rs");
    s.reply(PREPARE_CALLS, json!([remote, item("alpha", 12, "a.rs", 0)]));
    let found = s.prepare_calls().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "alpha");
    let mut odd = item("odd", 12, "b.rs", 0);
    odd["uri"] = json!("untitled:Untitled-1");
    s.reply(INCOMING, json!([{ "from": odd, "fromRanges": [] }]));
    let edges = answer(s.manager.incoming_calls(s.buffer.id(), &found[0])).unwrap();
    assert!(edges.is_empty());
}

#[test]
fn null_reply_and_timeout_are_empty() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, Value::Null);
    assert_eq!(s.prepare_calls(), Some(Vec::new()));

    s.reply(PREPARE_CALLS, json!([item("alpha", 12, "a.rs", 0)]));
    let target = s.prepare_calls().unwrap().remove(0);
    s.reply(INCOMING, Value::Null);
    assert_eq!(
        answer(s.manager.incoming_calls(s.buffer.id(), &target)),
        Ok(Vec::new())
    );

    // Past the references timeout of 30 s.
    s.fake.set_reply(
        OUTGOING,
        Scripted {
            reply: Reply::Result(json!([])),
            delay: Duration::from_secs(600),
        },
    );
    let mut late = s.manager.outgoing_calls(s.buffer.id(), &target).boxed();
    s.fake.wait_for("the late request", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { method, .. } if method == OUTGOING))
    });
    s.clock.wait_for_sleep(Duration::from_secs(30));
    s.clock.advance(Duration::from_secs(30));
    assert_eq!(answer(&mut late), Ok(Vec::new()));
}

#[test]
fn type_hierarchy_supertypes_and_subtypes_decode() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "struct Alpha;\n");
    s.reply(PREPARE_TYPES, json!([item("Alpha", 23, "a.rs", 0)]));
    let target = answer(
        s.manager
            .prepare_type_hierarchy(s.buffer.id(), s.buffer.version(), 1),
    )
    .unwrap()
    .unwrap()
    .remove(0);
    assert_eq!(target.kind, OutlineKind::Struct);
    s.reply(SUPERTYPES, json!([item("Base", 11, "b.rs", 1)]));
    s.reply(
        SUBTYPES,
        json!([item("Child", 5, "c.rs", 2), item("Other", 5, "d.rs", 3)]),
    );
    let up = answer(s.manager.supertypes(s.buffer.id(), &target)).unwrap();
    let down = answer(s.manager.subtypes(s.buffer.id(), &target)).unwrap();
    assert_eq!(up.len(), 1);
    assert_eq!(up[0].name, "Base");
    assert_eq!(up[0].kind, OutlineKind::Interface);
    assert_eq!(
        down.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        ["Child", "Other"]
    );
    assert_eq!(s.requests(SUPERTYPES)[0]["item"], target.raw);
    assert_eq!(s.requests(SUBTYPES)[0]["item"], target.raw);
}

#[test]
fn selection_in_converts_utf16_positions() {
    // The crab is four bytes and two UTF-16 units: `alpha` is bytes 5..10.
    let buffer = Buffer::new("🦀 alpha\n");
    let text = buffer.text();
    for (encoding, selection) in [
        (PositionEncoding::Utf16, range(0, 3, 8)),
        (PositionEncoding::Utf8, range(0, 5, 10)),
    ] {
        let one = HierarchyItem {
            name: "alpha".into(),
            detail: None,
            kind: OutlineKind::Function,
            path: PathBuf::from(ROOT).join("a.rs"),
            range: serde_json::from_value(range(
                0,
                0,
                if encoding == PositionEncoding::Utf16 {
                    8
                } else {
                    10
                },
            ))
            .unwrap(),
            selection_range: serde_json::from_value(selection).unwrap(),
            encoding,
            deprecated: false,
            raw: Value::Null,
            from: None,
        };
        assert_eq!(one.selection_in(text), 5..10);
        assert_eq!(one.range_in(text), 0..10);
    }
}

#[test]
fn edges_are_capped() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 12, "a.rs", 0)]));
    let target = s.prepare_calls().unwrap().remove(0);
    let many: Vec<Value> = (0..MAX_EDGES + 50)
        .map(|i| json!({ "from": item(&format!("c{i}"), 12, "b.rs", 1), "fromRanges": [] }))
        .collect();
    s.reply(INCOMING, Value::Array(many));
    let edges = answer(s.manager.incoming_calls(s.buffer.id(), &target)).unwrap();
    assert_eq!(edges.len(), MAX_EDGES);
}

#[test]
fn initialize_records_the_standard_type_provider() {
    for (experimental, expected) in [
        (None, json!({ "typeHierarchyProvider": true })),
        (
            Some(json!({ "a": 1 })),
            json!({ "a": 1, "typeHierarchyProvider": true }),
        ),
        (
            Some(json!(false)),
            json!({ "typeHierarchyProvider": true, "kept": false }),
        ),
        (
            Some(json!([1])),
            json!({ "typeHierarchyProvider": true, "kept": [1] }),
        ),
    ] {
        let mut raw = json!({ "capabilities": { "typeHierarchyProvider": true } });
        if let Some(experimental) = experimental {
            raw["capabilities"]["experimental"] = experimental;
        }
        raw["serverInfo"] = Value::Null;
        let mut result: lsp_types::InitializeResult = serde_json::from_value(raw.clone()).unwrap();
        crate::hierarchy::mark_type_hierarchy(&raw, &mut result.capabilities);
        assert_eq!(result.capabilities.experimental, Some(expected));
        assert!(crate::hierarchy::types(&result.capabilities));
    }
    // Absent or false: no marker.
    let mut result = lsp_types::InitializeResult::default();
    let raw = json!({ "capabilities": { "typeHierarchyProvider": false } });
    crate::hierarchy::mark_type_hierarchy(&raw, &mut result.capabilities);
    assert!(!crate::hierarchy::types(&result.capabilities));
}

#[test]
fn type_hierarchy_accepts_tag_arrays() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "struct Alpha;\n");
    let mut tagged = item("Alpha", 23, "a.rs", 0);
    tagged["tags"] = json!([1]);
    let mut empty = item("Empty", 23, "a.rs", 1);
    empty["tags"] = json!([]);
    s.reply(PREPARE_TYPES, json!([tagged, empty]));
    let found = answer(
        s.manager
            .prepare_type_hierarchy(s.buffer.id(), s.buffer.version(), 1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(found.len(), 2);
    assert!(found[0].deprecated);
    assert!(!found[1].deprecated);
    let mut up = item("Base", 11, "b.rs", 1);
    up["tags"] = json!([1]);
    s.reply(SUPERTYPES, json!([up, {"extra": 1}]));
    s.reply(
        SUBTYPES,
        json!([{ "name": "Kid", "kind": 23, "tags": [], "uri": uri("c.rs"),
        "range": range(0, 0, 1), "selectionRange": range(0, 0, 1) }]),
    );
    let supers = answer(s.manager.supertypes(s.buffer.id(), &found[0])).unwrap();
    assert_eq!(supers.len(), 1);
    assert!(supers[0].deprecated);
    let subs = answer(s.manager.subtypes(s.buffer.id(), &found[0])).unwrap();
    assert_eq!(subs.len(), 1);
    assert!(!subs[0].deprecated);
}

#[test]
fn unknown_fields_and_nulls_round_trip() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    let mut odd = item("alpha", 12, "a.rs", 0);
    odd["data"] = Value::Null;
    odd["vendor"] = json!({ "x": [1, null] });
    s.reply(PREPARE_CALLS, json!([odd.clone()]));
    let target = s.prepare_calls().unwrap().remove(0);
    assert_eq!(target.raw, odd);

    let mut caller = item("caller", 12, "b.rs", 1);
    caller["data"] = Value::Null;
    caller["vendor"] = json!(7);
    s.reply(
        INCOMING,
        json!([{ "from": caller.clone(), "fromRanges": [] }]),
    );
    s.reply(SUPERTYPES, json!([caller.clone()]));
    let edges = answer(s.manager.incoming_calls(s.buffer.id(), &target)).unwrap();
    assert_eq!(edges[0].item.raw, caller);
    // The result goes back as it came, to the same session.
    s.reply(OUTGOING, Value::Null);
    answer(s.manager.outgoing_calls(s.buffer.id(), &edges[0].item)).unwrap();
    assert_eq!(s.requests(INCOMING)[0]["item"], odd);
    assert_eq!(s.requests(OUTGOING)[0]["item"], caller);
    let up = answer(s.manager.supertypes(s.buffer.id(), &target)).unwrap();
    assert_eq!(up[0].raw, caller);
    assert_eq!(s.requests(SUPERTYPES)[0]["item"], odd);
}

#[test]
fn items_from_another_session_are_stale() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn alpha() {}\n");
    s.reply(PREPARE_CALLS, json!([item("alpha", 12, "a.rs", 0)]));
    let mut target = s.prepare_calls().unwrap().remove(0);
    s.reply(
        INCOMING,
        json!([{ "from": item("c", 12, "b.rs", 1), "fromRanges": [] }]),
    );
    let edges = answer(s.manager.incoming_calls(s.buffer.id(), &target)).unwrap();
    // A result keeps the session of the item it answers.
    assert_eq!(edges[0].item.from, target.from);
    let before = s.requests(INCOMING).len();
    target.from = target.from.map(|owner| crate::Owner {
        identity: (s.manager.identities().new_session(), owner.identity.1),
        ..owner
    });
    let stale = answer(s.manager.incoming_calls(s.buffer.id(), &target));
    assert_eq!(stale, Err(LspError::Unavailable("stale".into())));
    assert_eq!(s.requests(INCOMING).len(), before);
}
