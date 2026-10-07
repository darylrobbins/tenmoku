//! Document sync (§4.1) against the fake's replica: open and close, the
//! save barrier, the backstop tick, the full-text fallback, and random
//! edits round-tripped under each encoding.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use lsp_types::{
    PositionEncodingKind, SaveOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Url,
};
use serde_json::{Value, json};

use super::{Events, answer, manager};
use crate::fake::{Echo, FakeServer, InitializeReply, Record, Replica, Saved, Script};
use crate::server::BACKSTOP;
use crate::{LspManager, ManualClock, ServerId, ServerSpec, ServerStatus};

const ROOT: &str = "/fake/root";

/// A started server on the fake, with a manual clock, so the backstop tick
/// fires only when a test says.
struct Session {
    fake: FakeServer,
    manager: LspManager,
    clock: Arc<ManualClock>,
    server: ServerId,
    _events: Events,
}

/// A buffer synced to [`Session`]'s server.
struct Doc {
    buffer: Buffer,
    uri: Url,
}

impl Session {
    fn new(script: Script) -> Self {
        let fake = FakeServer::new(script);
        let clock = Arc::new(ManualClock::new());
        let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
        let server = manager
            .start(ServerSpec::new("sync", "fake-server"), Path::new(ROOT))
            .unwrap();
        assert_eq!(events.status(server), ServerStatus::Starting);
        fake.wait_initialized();
        Self {
            fake,
            manager,
            clock,
            server,
            _events: events,
        }
    }

    /// Opens a buffer read from `contents` as `name` under the root, and
    /// waits for its `didOpen`.
    fn open(&mut self, name: &str, contents: &str, language: &str) -> Doc {
        let mut buffer = Buffer::from_file_contents(contents);
        let path = PathBuf::from(ROOT).join(name);
        let subscription = buffer.subscribe();
        self.manager
            .open(
                self.server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                language,
            )
            .unwrap();
        let uri = Url::from_file_path(&path).unwrap();
        self.fake
            .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        Doc { buffer, uri }
    }

    /// Nudges `doc`'s server and waits until its replica has `version`.
    fn sync(&self, doc: &Doc, version: i32) -> Replica {
        self.manager.changed(doc.buffer.id());
        self.wait_version(doc, version)
    }

    fn wait_version(&self, doc: &Doc, version: i32) -> Replica {
        self.fake
            .wait_for_document(&doc.uri, &format!("version {version}"), |r| {
                r.is_some_and(|r| r.version >= version)
            })
            .unwrap()
    }

    /// Returns once the fake has read everything the actor sent before
    /// now: a request goes behind it, and the fake answers in order.
    fn barrier(&self) {
        answer(self.manager.request::<Echo>(self.server, json!({}))).unwrap();
    }

    /// The `didChange` and `didSave` params sent for `doc`, in order.
    fn sent(&self, doc: &Doc) -> Vec<(String, Value)> {
        self.fake
            .records()
            .into_iter()
            .filter_map(|record| match record {
                Record::Notification { method, params }
                    if params["textDocument"]["uri"] == json!(doc.uri) =>
                {
                    Some((method, params))
                }
                _ => None,
            })
            .collect()
    }

    fn assert_in_sync(&self, doc: &Doc) {
        let replica = self.fake.document(&doc.uri).unwrap();
        assert_eq!(replica.text, doc.buffer.text().to_string());
        assert_eq!(self.fake.sync_errors(), Vec::<String>::new());
    }
}

impl Doc {
    fn edit(&mut self, edits: Vec<(Range<usize>, &str)>) {
        self.buffer
            .edit(edits, Origin::User)
            .expect("the edit changed something");
    }
}

fn script(encoding: Option<PositionEncodingKind>, sync: TextDocumentSyncCapability) -> Script {
    let mut result = crate::fake::initialize_result(encoding);
    result.capabilities.text_document_sync = Some(sync);
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

fn incremental(save: TextDocumentSyncSaveOptions) -> TextDocumentSyncCapability {
    TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
        open_close: Some(true),
        change: Some(TextDocumentSyncKind::INCREMENTAL),
        will_save: None,
        will_save_wait_until: None,
        save: Some(save),
    })
}

fn methods(sent: &[(String, Value)]) -> Vec<&str> {
    sent.iter().map(|(method, _)| method.as_str()).collect()
}

fn changes(params: &Value) -> &Vec<Value> {
    params["contentChanges"].as_array().unwrap()
}

fn has_range(change: &Value) -> bool {
    change.get("range").is_some_and(|range| !range.is_null())
}

#[test]
fn open_and_close() {
    let mut session = Session::new(Script::default());
    let mut a = session.open("src/a.rs", "fn a() {}\r\nfn b() {}\r\n", "rust");
    let mut b = session.open("web/app.tsx", "let x = <b/>;\n", "tsx");

    let opens: Vec<Value> = session
        .fake
        .records()
        .into_iter()
        .filter_map(|record| match record {
            Record::Notification { method, params } if method == "textDocument/didOpen" => {
                Some(params["textDocument"].clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        opens,
        [
            json!({
                "uri": a.uri,
                "languageId": "rust",
                "version": 0,
                "text": "fn a() {}\nfn b() {}\n",
            }),
            json!({
                "uri": b.uri,
                "languageId": "typescriptreact",
                "version": 0,
                "text": "let x = <b/>;\n",
            }),
        ]
    );

    // Each document syncs from its own subscription.
    a.edit(vec![(3..4, "main")]);
    session.sync(&a, 1);
    assert_eq!(session.fake.document(&b.uri).unwrap().version, 0);
    session.assert_in_sync(&a);
    b.edit(vec![(4..5, "y")]);
    session.sync(&b, 1);
    assert_eq!(session.fake.document(&a.uri).unwrap().version, 1);
    session.assert_in_sync(&b);

    // An edit not yet drained when it closes is sent before didClose.
    a.edit(vec![(0..0, "// kept\n")]);
    session.manager.close(a.buffer.id());
    // Edits after close send nothing, even before the server's thread has
    // closed it, nudged or at the backstop tick.
    a.edit(vec![(0..0, "// gone\n")]);
    session.manager.changed(a.buffer.id());
    let closed = session
        .fake
        .wait_for_document(&a.uri, "didClose", |r| r.is_some_and(|r| !r.open))
        .unwrap();
    assert_eq!(closed.version, 2);
    assert_eq!(closed.text, "// kept\nfn main() {}\nfn b() {}\n");
    session.clock.advance(BACKSTOP);
    b.edit(vec![(0..0, "// here\n")]);
    session.sync(&b, 2);
    assert_eq!(
        methods(&session.sent(&a)),
        [
            "textDocument/didOpen",
            "textDocument/didChange",
            "textDocument/didChange",
            "textDocument/didClose"
        ]
    );
    session.assert_in_sync(&b);
    assert_eq!(session.fake.sync_errors(), Vec::<String>::new());
}

#[test]
fn save_barrier_orders_changes_and_saves() {
    let mut session = Session::new(Script::default());
    let mut doc = session.open("a.txt", "one\n", "text");

    // Edited(V1), Saved(V1), Edited(V2), Saved(V2), Edited(V3) in one drain.
    doc.edit(vec![(0..0, "1")]);
    doc.buffer.did_save();
    doc.edit(vec![(0..0, "2")]);
    doc.buffer.did_save();
    doc.edit(vec![(0..0, "3")]);
    let replica = session.sync(&doc, 3);
    let sent = session.sent(&doc);
    assert_eq!(
        methods(&sent[1..]),
        [
            "textDocument/didChange",
            "textDocument/didSave",
            "textDocument/didChange",
            "textDocument/didSave",
            "textDocument/didChange",
        ]
    );
    let versions: Vec<&Value> = sent[1..]
        .iter()
        .filter(|(method, _)| method == "textDocument/didChange")
        .map(|(_, params)| &params["textDocument"]["version"])
        .collect();
    assert_eq!(versions, [&json!(1), &json!(2), &json!(3)]);
    assert_eq!(
        replica.saves,
        [
            Saved {
                version: 1,
                text: None
            },
            Saved {
                version: 2,
                text: None
            },
        ]
    );
    session.assert_in_sync(&doc);

    // Two consecutive batches become one didChange.
    doc.edit(vec![(0..0, "a")]);
    doc.edit(vec![(5..5, "b")]);
    session.sync(&doc, 4);
    let sent = session.sent(&doc);
    let (method, params) = sent.last().unwrap();
    assert_eq!(method, "textDocument/didChange");
    assert_eq!(params["textDocument"]["version"], 4);
    assert_eq!(changes(params).len(), 2, "{params:#}");
    assert_eq!(sent.len(), 7);
    session.assert_in_sync(&doc);

    // With includeText, didSave carries the saved version's text, not a
    // later one.
    let mut session = Session::new(script(
        Some(PositionEncodingKind::UTF8),
        incremental(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
            include_text: Some(true),
        })),
    ));
    let mut doc = session.open("b.txt", "one\n", "text");
    doc.edit(vec![(0..0, "saved ")]);
    doc.buffer.did_save();
    doc.edit(vec![(0..0, "later ")]);
    let replica = session.sync(&doc, 2);
    assert_eq!(
        replica.saves,
        [Saved {
            version: 1,
            text: Some("saved one\n".into()),
        }]
    );
    assert_eq!(replica.text, "later saved one\n");
    session.assert_in_sync(&doc);
}

#[test]
fn backstop_tick_delivers_without_a_nudge() {
    let mut session = Session::new(Script::default());
    let mut doc = session.open("a.txt", "one\n", "text");
    doc.edit(vec![(0..0, "1")]);
    session.sync(&doc, 1);

    // A save with no nudge waits for the tick, armed when the document
    // opened.
    doc.buffer.did_save();
    session.clock.wait_for_sleep(BACKSTOP);
    session.clock.advance(BACKSTOP - Duration::from_millis(1));
    session.barrier();
    assert_eq!(session.fake.document(&doc.uri).unwrap().saves, []);
    session.clock.advance(Duration::from_millis(1));
    session.fake.wait_for_document(&doc.uri, "didSave", |r| {
        r.is_some_and(|r| r.saves.len() == 1)
    });

    // Nudged, it goes without the clock moving.
    session.clock.wait_for_sleep(BACKSTOP);
    doc.edit(vec![(0..0, "2")]);
    session.sync(&doc, 2);
    doc.buffer.did_save();
    session.manager.changed(doc.buffer.id());
    let replica = session
        .fake
        .wait_for_document(&doc.uri, "didSave", |r| {
            r.is_some_and(|r| r.saves.len() == 2)
        })
        .unwrap();
    assert_eq!(
        replica.saves[1],
        Saved {
            version: 2,
            text: None
        }
    );
    assert_eq!(session.clock.sleeping(), [BACKSTOP]);
    session.assert_in_sync(&doc);
}

#[test]
fn full_text_fallback() {
    let whole = |params: &Value| -> Option<String> {
        match &changes(params)[..] {
            [change] if !has_range(change) => Some(change["text"].as_str().unwrap().to_owned()),
            _ => None,
        }
    };
    let last_change = |session: &Session, doc: &Doc| -> Value {
        let sent = session.sent(doc);
        let (method, params) = sent.last().unwrap();
        assert_eq!(method, "textDocument/didChange");
        params.clone()
    };

    // A server that syncs Full.
    let mut session = Session::new(script(
        None,
        TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL),
    ));
    let mut doc = session.open("full.txt", "one\ntwo\n", "text");
    doc.edit(vec![(4..7, "2")]);
    session.sync(&doc, 1);
    assert_eq!(
        whole(&last_change(&session, &doc)).as_deref(),
        Some("one\n2\n")
    );
    session.assert_in_sync(&doc);

    // 64 edits in a drain stay incremental; 65 go whole, even split by a
    // save.
    let mut session = Session::new(Script::default());
    let mut doc = session.open("many.txt", &"abcd".repeat(100), "text");
    doc.edit((0..64).map(|i| (i * 4..i * 4 + 1, "x")).collect());
    session.sync(&doc, 1);
    let params = last_change(&session, &doc);
    assert_eq!(changes(&params).len(), 64);
    assert!(changes(&params).iter().all(has_range));
    session.assert_in_sync(&doc);
    doc.edit((0..40).map(|i| (i * 4 + 1..i * 4 + 2, "y")).collect());
    doc.edit((0..25).map(|i| (i * 4 + 2..i * 4 + 3, "z")).collect());
    session.sync(&doc, 2);
    let text = doc.buffer.text().to_string();
    assert_eq!(whole(&last_change(&session, &doc)), Some(text));
    session.assert_in_sync(&doc);
    doc.edit((0..40).map(|i| (i * 4 + 3..i * 4 + 4, "w")).collect());
    doc.buffer.did_save();
    doc.edit((0..25).map(|i| (i * 4..i * 4 + 1, "v")).collect());
    session.sync(&doc, 4);
    let sent = session.sent(&doc);
    let tail: Vec<_> = sent[sent.len() - 3..]
        .iter()
        .map(|(method, params)| {
            let change = method == "textDocument/didChange";
            (method.as_str(), change && whole(params).is_some())
        })
        .collect();
    assert_eq!(
        tail,
        [
            ("textDocument/didChange", true),
            ("textDocument/didSave", false),
            ("textDocument/didChange", true),
        ]
    );
    session.assert_in_sync(&doc);

    // New text of exactly half the document stays incremental; more goes
    // whole.
    let mut doc = session.open("half.txt", "abcd", "text");
    doc.edit(vec![(4..4, "efgh")]);
    session.sync(&doc, 1);
    assert_eq!(whole(&last_change(&session, &doc)), None);
    session.assert_in_sync(&doc);
    doc.edit(vec![(0..0, "01234")]);
    doc.edit(vec![(13..13, "56789")]);
    session.sync(&doc, 2);
    assert_eq!(
        whole(&last_change(&session, &doc)).as_deref(),
        Some("01234abcdefgh56789")
    );
    session.assert_in_sync(&doc);
}

/// Strings the random edits insert: the §4.4 cases' texts, line breaks
/// and plain ASCII.
const FRAGMENTS: &[&str] = &[
    "a\u{1F600}b",
    "\u{1F600}\u{1F600}x",
    "x\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}y",
    "\u{1F44D}\u{1F3FD}x",
    "\u{1F1E8}\u{1F1E6}x",
    "\u{2764}\u{FE0F}x",
    "e\u{301}x",
    "\u{E9}x",
    "\u{4E2D}x",
    "\n",
    "\n\n",
    "a\n\u{1F600}b\n",
    "word",
    "",
];

/// A small deterministic generator (SplitMix64), so a failing case can be
/// rerun from its seed. fe-lsp has no proptest dependency, and this card
/// adds none.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn fragment(&mut self) -> &'static str {
        FRAGMENTS[self.below(FRAGMENTS.len())]
    }
}

/// How many random documents each round trip syncs: `PROPTEST_CASES`
/// (as `scripts/soak.sh` sets it), else a few, keeping the gate fast.
fn case_count() -> u64 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(24)
}

/// Random batches of insertions and deletions of [`FRAGMENTS`], several
/// edits per batch at times, and several batches per drain at times, on
/// buffers synced to a fake negotiating `encoding`. After every
/// `didChange` the fake's replica, built with its own converter, must
/// equal the buffer's text.
fn round_trip(encoding: Option<PositionEncodingKind>) {
    let new_session = || {
        Session::new(script(
            encoding.clone(),
            incremental(TextDocumentSyncSaveOptions::Supported(true)),
        ))
    };
    let mut session = new_session();
    let (mut changes_sent, mut ranged) = (0, 0);
    for case in 0..case_count() {
        if case > 0 && case % CASES_PER_SESSION == 0 {
            // The fake keeps every record and replica: start afresh now
            // and then, so a long soak doesn't grow without bound.
            (changes_sent, ranged) = count_changes(&session, changes_sent, ranged);
            session = new_session();
        }
        let seed = 0x5EED ^ case.wrapping_mul(0x9E37_79B9);
        let mut rng = Rng(seed);
        let start: String = (0..60).map(|_| rng.fragment()).collect();
        let mut doc = session.open(&format!("case{case}.txt"), &start, "text");
        let mut history = vec![format!("seed {seed:#x}: open {start:?}")];
        let mut version = 0;
        for _ in 0..12 {
            let before = doc.buffer.version();
            for _ in 0..1 + rng.below(3) {
                let edits = random_batch(&mut rng, &doc.buffer.text().to_string());
                history.push(format!("edit {edits:?}"));
                // An edit that changes nothing makes no batch.
                let _ = doc.buffer.edit(edits, Origin::User);
            }
            session.manager.changed(doc.buffer.id());
            session.barrier();
            // One drain, so one didChange if anything changed.
            version += i32::from(doc.buffer.version() != before);
            let replica = session.fake.document(&doc.uri).unwrap();
            let history = history.join("\n");
            assert_eq!(replica.version, version, "{history}");
            assert_eq!(replica.text, doc.buffer.text().to_string(), "{history}");
            assert_eq!(
                session.fake.sync_errors(),
                Vec::<String>::new(),
                "{history}"
            );
        }
        session.manager.close(doc.buffer.id());
    }
    let (changes_sent, ranged) = count_changes(&session, changes_sent, ranged);
    // Most go incremental, so the replica's converter is what's tested.
    assert!(
        ranged * 4 >= changes_sent * 3,
        "only {ranged} of {changes_sent} didChanges had ranges"
    );
}

const CASES_PER_SESSION: u64 = 64;

/// Adds `session`'s didChanges, and those of them sent as ranges, to the
/// counts so far.
fn count_changes(session: &Session, mut sent: usize, mut ranged: usize) -> (usize, usize) {
    for record in session.fake.records() {
        if let Record::Notification { method, params } = record
            && method == "textDocument/didChange"
        {
            sent += 1;
            ranged += usize::from(changes(&params).iter().all(has_range));
        }
    }
    (sent, ranged)
}

/// One to three edits on char boundaries of `text`, sorted and
/// non-overlapping: insertions, and deletions or replacements of up to five
/// chars (which can join rows).
fn random_batch(rng: &mut Rng, text: &str) -> Vec<(Range<usize>, &'static str)> {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .collect();
    let mut starts: Vec<usize> = (0..1 + rng.below(3))
        .map(|_| rng.below(boundaries.len()))
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let mut edits = Vec::new();
    for (i, &start) in starts.iter().enumerate() {
        let limit = starts.get(i + 1).copied().unwrap_or(boundaries.len() - 1);
        let end = (start + rng.below(6)).min(limit);
        edits.push((boundaries[start]..boundaries[end], rng.fragment()));
    }
    edits
}

#[test]
fn round_trip_utf8() {
    round_trip(Some(PositionEncodingKind::UTF8));
}

#[test]
fn round_trip_utf16() {
    round_trip(Some(PositionEncodingKind::UTF16));
}

#[test]
fn events_before_the_opened_version_are_skipped() {
    let mut session = Session::new(Script::default());
    let mut buffer = Buffer::new("one\n");
    let subscription = buffer.subscribe();
    buffer.edit([(0..0, "1")], Origin::User).unwrap();
    buffer.did_save();
    let path = PathBuf::from(ROOT).join("late.txt");
    session
        .manager
        .open(
            session.server,
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            &path,
            "text",
        )
        .unwrap();
    let mut doc = Doc {
        buffer,
        uri: Url::from_file_path(&path).unwrap(),
    };
    doc.edit(vec![(0..0, "2")]);
    let replica = session.sync(&doc, 1);
    assert_eq!(replica.text, "21one\n");
    // The save of the opened version itself still goes.
    assert_eq!(
        methods(&session.sent(&doc)),
        [
            "textDocument/didOpen",
            "textDocument/didSave",
            "textDocument/didChange"
        ]
    );
    session.assert_in_sync(&doc);
}

/// A server on the fake whose `initialize` answer waits for the returned
/// clock, so commands made meanwhile queue behind it.
fn starting(script: Script) -> (FakeServer, Arc<ManualClock>, LspManager, ServerId, Events) {
    let fake_clock = Arc::new(ManualClock::new());
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: Duration::from_secs(1),
            ..script
        },
        fake_clock.clone(),
    );
    let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    let server = manager
        .start(ServerSpec::new("sync", "fake-server"), Path::new(ROOT))
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake_clock.wait_for_sleep(Duration::from_secs(1));
    (fake, fake_clock, manager, server, events)
}

fn open_buffer(
    manager: &mut LspManager,
    server: ServerId,
    buffer: &mut Buffer,
    name: &str,
) -> Result<Url, crate::LspError> {
    let path = PathBuf::from(ROOT).join(name);
    let subscription = buffer.subscribe();
    manager.open(
        server,
        buffer.id(),
        buffer.text().clone(),
        buffer.version(),
        subscription,
        &path,
        "text",
    )?;
    Ok(Url::from_file_path(&path).unwrap())
}

#[test]
fn close_keeps_order_with_nudges_and_requests() {
    let (fake, fake_clock, mut manager, server, _events) = starting(Script::default());
    // All made while the server starts, so none is carried out before
    // close or the reopen.
    let mut a = Buffer::new("a\n");
    let a_uri = open_buffer(&mut manager, server, &mut a, "a.txt").unwrap();
    a.edit([(0..0, "1")], Origin::User).unwrap();
    manager.changed(a.id());
    let first = manager.request::<Echo>(server, json!({ "n": 1 }));
    manager.close(a.id());
    let mut b = Buffer::new("b\n");
    let b_uri = open_buffer(&mut manager, server, &mut b, "b.txt").unwrap();
    b.edit([(0..0, "2")], Origin::User).unwrap();
    manager.changed(b.id());
    let second = manager.request::<Echo>(server, json!({ "n": 2 }));
    open_buffer(&mut manager, server, &mut b, "b.txt").unwrap();

    fake_clock.advance(Duration::from_secs(1));
    answer(first).unwrap();
    answer(second).unwrap();
    fake.wait_for_document(&b_uri, "the reopen", |r| {
        r.is_some_and(|r| r.open && r.version == 0)
    });
    let order: Vec<String> = fake
        .records()
        .into_iter()
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == "fake/echo" => {
                Some(format!("echo {}", params["n"]))
            }
            Record::Notification { method, params } if method.starts_with("textDocument/") => {
                let uri = params["textDocument"]["uri"].as_str().unwrap().to_owned();
                let name = if uri == a_uri.as_str() { "a" } else { "b" };
                Some(format!("{} {name}", &method["textDocument/".len()..]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        [
            "didOpen a",
            "didChange a",
            "echo 1",
            "didClose a",
            "didOpen b",
            "didChange b",
            "echo 2",
            "didClose b",
            "didOpen b",
        ]
    );
    assert_eq!(fake.document(&b_uri).unwrap().text, "2b\n");
    assert_eq!(fake.sync_errors(), Vec::<String>::new());
}

#[test]
fn subscriptions_go_with_the_server() {
    // A crash after opening.
    let mut session = Session::new(Script::default());
    let mut doc = session.open("a.txt", "one\n", "text");
    assert!(session.manager.is_subscribed(doc.buffer.id()));
    session.fake.crash();
    super::wait_until("the subscription to go", || {
        !session.manager.is_subscribed(doc.buffer.id())
    });
    doc.edit(vec![(0..0, "1")]);
    assert!(!session.manager.is_subscribed(doc.buffer.id()));

    // An open queued while a start fails.
    let (_fake, fake_clock, mut manager, server, events) = starting(Script {
        initialize: InitializeReply::Error {
            code: -32603,
            message: "no".into(),
        },
        ..Script::default()
    });
    let mut buffer = Buffer::new("two\n");
    open_buffer(&mut manager, server, &mut buffer, "b.txt").unwrap();
    assert!(manager.is_subscribed(buffer.id()));
    fake_clock.advance(Duration::from_secs(1));
    assert!(matches!(
        events.status(server),
        ServerStatus::Stopped(Some(_))
    ));
    super::wait_until("the subscription to go", || {
        !manager.is_subscribed(buffer.id())
    });
}

#[test]
fn backstop_drains_wait_for_room() {
    let mut session = Session::new(Script::default());
    let mut doc = session.open("a.txt", "one\n", "text");
    session.clock.wait_for_sleep(BACKSTOP);
    session.fake.set_writes(crate::fake::Writes::Held);
    let mut reply = None;
    for i in 0..100 {
        doc.edit(vec![(0..0, "x")]);
        if i == 50 {
            reply = Some(session.manager.request::<Echo>(session.server, json!({})));
        }
        session.clock.advance(BACKSTOP);
        session.clock.wait_for_sleep(BACKSTOP);
    }
    session.fake.set_writes(crate::fake::Writes::Open);
    // The request queued behind the drains is carried out.
    answer(reply.unwrap()).unwrap();
    session.manager.changed(doc.buffer.id());
    session.barrier();
    session.assert_in_sync(&doc);
    // Ticks with no room made no notifications: about a window's worth
    // went out before the writes stopped, then the rest as one.
    let changes = methods(&session.sent(&doc))
        .iter()
        .filter(|method| **method == "textDocument/didChange")
        .count();
    assert!(
        changes <= crate::server::QUEUED_MAX as usize + 4,
        "{changes} didChanges"
    );
}
