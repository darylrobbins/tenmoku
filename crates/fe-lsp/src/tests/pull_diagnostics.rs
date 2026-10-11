//! Pull diagnostics (pull.rs): when documents are pulled, which version a
//! reply is decoded for, how pushed and pulled lists merge, and which
//! replies are dropped.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, Edit, Origin, Version};
use fe_text::Text;
use lsp_types::notification::PublishDiagnostics;
use lsp_types::request::WorkspaceDiagnosticRefresh;
use lsp_types::{
    DiagnosticOptions, DiagnosticServerCapabilities, NumberOrString, Position,
    PositionEncodingKind, PublishDiagnosticsParams, Range, Url,
};
use serde_json::{Value, json};

use super::{Events, ROOT, answer, manager, wait_until};
use crate::document::{RING, SentVersion};
use crate::fake::{
    Echo, FakeServer, InitializeReply, Record, Reply, Script, Scripted, Writes, initialize_result,
};
use crate::pull::{Lists, Source};
use crate::{
    Diagnostic, IncarnationId, LspEvent, LspManager, ManualClock, PositionEncoding, ServerId,
    ServerSpec, ServerStatus, SessionId,
};

const PULL: &str = "textDocument/diagnostic";
/// How long, on the fake's clock, a delayed reply waits.
const DELAY: Duration = Duration::from_secs(1);

/// A server on the fake, which pulls if it was asked to advertise
/// `diagnosticProvider`. The manager's clock moves only when a test moves
/// it, and so does the fake's, which delayed pull replies wait on.
struct Pulling {
    fake: FakeServer,
    /// The fake's clock.
    replies: Arc<ManualClock>,
    /// The manager's clock.
    clock: Arc<ManualClock>,
    manager: LspManager,
    server: ServerId,
    events: Events,
}

impl Pulling {
    fn new(provider: bool) -> Self {
        let mut result = initialize_result(Some(PositionEncodingKind::UTF8));
        if provider {
            result.capabilities.diagnostic_provider =
                Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                    identifier: Some("fake".into()),
                    inter_file_dependencies: true,
                    workspace_diagnostics: false,
                    ..Default::default()
                }));
        }
        let script = Script {
            initialize: InitializeReply::Result(Box::new(result)),
            ..Script::default()
        };
        let replies = Arc::new(ManualClock::new());
        let fake = FakeServer::with_clock(script, replies.clone());
        fake.set_reply(PULL, Scripted::now(full(Vec::new())));
        let clock = Arc::new(ManualClock::new());
        let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        assert_eq!(events.status(server), ServerStatus::Starting);
        fake.wait_initialized();
        Self {
            fake,
            replies,
            clock,
            manager,
            server,
            events,
        }
    }

    /// `initialize`'s params, as the fake got them.
    fn initialize_params(&self) -> Value {
        (self.fake.records().into_iter())
            .find_map(|record| match record {
                Record::Request { method, params, .. } if method == "initialize" => Some(params),
                _ => None,
            })
            .unwrap()
    }

    /// Answers pulls with `reply` after `delay` on the fake's clock, from
    /// the next pull the fake gets.
    fn reply(&self, reply: Reply, delay: Duration) {
        self.fake.set_reply(PULL, Scripted { reply, delay });
    }

    /// Lets every delayed reply waiting now go.
    fn release(&self) {
        self.replies.wait_for_sleep(DELAY);
        self.replies.advance(DELAY);
    }

    /// Opens `buffer` as `name` under the root and waits for its `didOpen`.
    fn open(&mut self, buffer: &mut Buffer, name: &str) -> Url {
        let path = PathBuf::from(ROOT).join(name);
        let uri = Url::from_file_path(&path).unwrap();
        let opens = |records: &[Record]| {
            (records.iter())
                .filter(|r| {
                    matches!(r, Record::Notification { method, params }
                        if method == "textDocument/didOpen"
                            && params["textDocument"]["uri"] == json!(uri))
                })
                .count()
        };
        let before = opens(&self.fake.records());
        let subscription = buffer.subscribe();
        self.manager
            .open_on(
                self.server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                "text",
            )
            .unwrap();
        self.fake
            .wait_for("didOpen", |records| opens(records) > before);
        uri
    }

    /// Opens `buffer` as `name` under the root, without waiting.
    fn open_unsynced(&mut self, buffer: &mut Buffer, name: &str) -> Url {
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
                "text",
            )
            .unwrap();
        Url::from_file_path(&path).unwrap()
    }

    /// Edits `buffer`, nudges its server and waits until the replica at
    /// `uri` has `version`.
    fn edit(
        &self,
        buffer: &mut Buffer,
        uri: &Url,
        edit: (std::ops::Range<usize>, &str),
        version: i32,
    ) {
        buffer.edit([edit], Origin::User).unwrap();
        self.manager.changed(buffer.id());
        self.fake
            .wait_for_document(uri, &format!("version {version}"), |r| {
                r.is_some_and(|r| r.version == version)
            });
    }

    /// Saves `buffer`, nudges its server and waits for the replica at
    /// `uri` to have `saves` saves.
    fn save(&self, buffer: &mut Buffer, uri: &Url, saves: usize) {
        buffer.did_save();
        self.manager.changed(buffer.id());
        self.fake
            .wait_for_document(uri, &format!("save {saves}"), |r| {
                r.is_some_and(|r| r.saves.len() == saves)
            });
    }

    /// The params of every pull of `uri` the fake got, in order.
    fn pulls(&self, uri: &Url) -> Vec<Value> {
        pulls_in(&self.fake.records(), uri)
    }

    /// Waits until the fake has got `n` pulls of `uri`, and returns them.
    fn wait_pulls(&self, uri: &Url, n: usize) -> Vec<Value> {
        let records = self
            .fake
            .wait_for(&format!("{n} pulls of {uri}"), |records| {
                pulls_in(records, uri).len() >= n
            });
        let pulls = pulls_in(&records, uri);
        assert_eq!(pulls.len(), n, "pulls of {uri}");
        pulls
    }

    /// Waits until the fake has answered every pull it got, then until it
    /// has read everything the actor sent before now.
    fn settled(&self) {
        self.fake.wait_for("every pull answered", |records| {
            let count = |replied: bool| {
                (records.iter())
                    .filter(|r| match r {
                        Record::Request { method, .. } => !replied && method == PULL,
                        Record::Replied { method, .. } => replied && method == PULL,
                        _ => false,
                    })
                    .count()
            };
            count(true) == count(false)
        });
        self.barrier();
    }

    /// Returns once the fake has read everything the actor sent before
    /// now.
    fn barrier(&self) {
        answer(self.manager.request::<Echo>(self.server, json!({}))).unwrap();
    }

    /// `buffer`'s active identity on the server.
    fn identity(&self, buffer: &Buffer) -> (SessionId, IncarnationId) {
        self.manager
            .active_identity(buffer.id(), self.server)
            .unwrap()
    }
}

/// The ids of the pulls of `uri`, in order.
fn pulls_ids(records: &[Record], uri: &Url) -> Vec<NumberOrString> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { id, method, params }
                if method == PULL && params["textDocument"]["uri"] == json!(uri) =>
            {
                Some(id.clone())
            }
            _ => None,
        })
        .collect()
}

fn pulls_in(records: &[Record], uri: &Url) -> Vec<Value> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. }
                if method == PULL && params["textDocument"]["uri"] == json!(uri) =>
            {
                Some(params.clone())
            }
            _ => None,
        })
        .collect()
}

/// A full report of `items`.
fn full(items: Vec<lsp_types::Diagnostic>) -> Reply {
    Reply::Result(json!({ "kind": "full", "items": items }))
}

fn unchanged() -> Reply {
    Reply::Result(json!({ "kind": "unchanged", "resultId": "1" }))
}

fn error(code: i32) -> Reply {
    Reply::Error {
        code,
        message: "not now".into(),
    }
}

/// The utf-8 LSP position of byte `offset` of `text`.
fn position(text: &str, offset: usize) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = before.len() - before.rfind('\n').map_or(0, |at| at + 1);
    Position::new(line as u32, column as u32)
}

/// A diagnostic over the first `word` in `text`, carrying `message`.
fn over(text: &str, word: &str, message: &str) -> lsp_types::Diagnostic {
    let start = text.find(word).expect("the word is in the text");
    lsp_types::Diagnostic {
        range: Range::new(position(text, start), position(text, start + word.len())),
        message: message.into(),
        ..Default::default()
    }
}

/// What a [`LspEvent::Diagnostics`] carried.
#[derive(Debug)]
struct Got {
    buffer: fe_buffer::BufferId,
    identity: (SessionId, IncarnationId),
    version: Version,
    items: Vec<Diagnostic>,
}

impl Got {
    /// Each item as the text it covers in `text` and its message.
    fn words(&self, text: &str) -> Vec<(String, String)> {
        (self.items.iter())
            .map(|item| (text[item.range.clone()].to_owned(), item.message.clone()))
            .collect()
    }
}

/// The next diagnostics the sink gets.
fn next(events: &Events) -> Got {
    events.until("diagnostics", |event| match event {
        LspEvent::Diagnostics {
            buffer,
            session,
            incarnation,
            version,
            items,
            ..
        } => Some(Got {
            buffer,
            identity: (session, incarnation),
            version,
            items,
        }),
        _ => None,
    })
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    (pairs.iter())
        .map(|(word, message)| ((*word).to_owned(), (*message).to_owned()))
        .collect()
}

/// The methods the fake got for `uri`, in order, without `textDocument/`.
fn methods(fake: &FakeServer, uri: &Url) -> Vec<String> {
    (fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } | Record::Notification { method, params }
                if params["textDocument"]["uri"] == json!(uri) =>
            {
                Some(method["textDocument/".len()..].to_owned())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn pulls_after_open_change_and_save() {
    let mut s = Pulling::new(true);
    let init = s.initialize_params();
    assert_eq!(
        init["capabilities"]["textDocument"]["diagnostic"],
        json!({ "dynamicRegistration": false, "relatedDocumentSupport": false }),
    );
    assert_eq!(
        init["capabilities"]["workspace"]["diagnostics"],
        json!({ "refreshSupport": true }),
    );
    assert!(
        init["capabilities"]["workspace"]
            .get("diagnostic")
            .is_none()
    );

    // didOpen, then a pull, answered for version 0 in its text.
    let v0 = "let alpha = 1;\n";
    s.reply(full(vec![over(v0, "alpha", "at 0")]), DELAY);
    let mut buffer = Buffer::new(v0);
    let at0 = buffer.version();
    let uri = s.open(&mut buffer, "a.rs");
    let pulls = s.wait_pulls(&uri, 1);
    assert_eq!(pulls[0]["identifier"], "fake");
    // Left out, not null: typescript-go refuses a null.
    let mut keys: Vec<_> = pulls[0].as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["identifier", "textDocument"]);
    s.release();
    let got = next(&s.events);
    assert_eq!(
        (got.buffer, got.identity, got.version),
        (buffer.id(), s.identity(&buffer), at0)
    );
    assert_eq!(got.words(v0), pairs(&[("alpha", "at 0")]));

    // A change while a pull is in flight is pulled once, after its reply.
    let v1 = "// one\nlet alpha = 1;\n";
    s.reply(full(vec![over(v1, "alpha", "at 1")]), DELAY);
    s.edit(&mut buffer, &uri, (0..0, "// one\n"), 1);
    let at1 = buffer.version();
    s.wait_pulls(&uri, 2);
    let v2 = "// one\nlet alpha = 12;\n";
    s.reply(full(vec![over(v2, "12", "at 2")]), Duration::ZERO);
    let one = v1.find('1').unwrap();
    s.edit(&mut buffer, &uri, (one + 1..one + 1, "2"), 2);
    assert_eq!(buffer.text().to_string(), v2);
    let at2 = buffer.version();
    s.barrier();
    assert_eq!(s.pulls(&uri).len(), 2, "a pull while one is in flight");
    s.release();
    // Version 1's reply, decoded in version 1's text...
    let got = next(&s.events);
    assert_eq!(got.version, at1);
    assert_eq!(got.words(v1), pairs(&[("alpha", "at 1")]));
    // ... then the change it held back, pulled once.
    let got = next(&s.events);
    assert_eq!(got.version, at2);
    assert_eq!(got.words(v2), pairs(&[("12", "at 2")]));
    s.settled();
    assert_eq!(s.pulls(&uri).len(), 3);

    // didSave.
    s.reply(full(vec![over(v2, "alpha", "saved")]), Duration::ZERO);
    s.save(&mut buffer, &uri, 1);
    s.wait_pulls(&uri, 4);
    let got = next(&s.events);
    assert_eq!(got.version, at2);
    assert_eq!(got.words(v2), pairs(&[("alpha", "saved")]));

    // Each pull went right behind what it followed.
    s.settled();
    assert_eq!(
        methods(&s.fake, &uri),
        [
            "didOpen",
            "diagnostic",
            "didChange",
            "diagnostic",
            "didChange",
            "diagnostic",
            "didSave",
            "diagnostic"
        ],
    );

    // A server without diagnosticProvider is never pulled, not even when
    // it asks for a refresh.
    let mut s = Pulling::new(false);
    let mut buffer = Buffer::new(v0);
    let uri = s.open(&mut buffer, "a.rs");
    s.edit(&mut buffer, &uri, (0..0, "// one\n"), 1);
    s.save(&mut buffer, &uri, 1);
    assert_eq!(
        s.fake.request::<WorkspaceDiagnosticRefresh>(()),
        Ok(Value::Null)
    );
    s.barrier();
    assert_eq!(methods(&s.fake, &uri), ["didOpen", "didChange", "didSave"]);
    s.events.assert_quiet();
}

#[test]
fn pushed_and_pulled_merge() {
    let mut s = Pulling::new(true);
    let v0 = "alpha beta gamma\n";
    let mut buffer = Buffer::new(v0);
    let at0 = buffer.version();
    let uri = s.open(&mut buffer, "a.rs");
    // The pull after didOpen: nothing.
    assert!(next(&s.events).items.is_empty());

    // A push for version 0.
    let push = |version: i32, diagnostics| {
        s.fake
            .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                uri: uri.clone(),
                diagnostics,
                version: Some(version),
            });
    };
    push(
        0,
        vec![over(v0, "alpha", "pushed"), over(v0, "beta", "pushed")],
    );
    let got = next(&s.events);
    assert_eq!(got.version, at0);
    assert_eq!(
        got.words(v0),
        pairs(&[("alpha", "pushed"), ("beta", "pushed")])
    );

    // Two edits later, a pull: one event at version 2 holds both, the
    // push mapped through both edits (an insertion before it, and a
    // replacement of exactly its second range).
    s.reply(error(-32802), Duration::ZERO);
    s.edit(&mut buffer, &uri, (0..0, "XX "), 1);
    s.settled();
    let v2 = "XX alpha BETA! gamma\n";
    s.reply(full(vec![over(v2, "gamma", "pulled")]), Duration::ZERO);
    s.edit(&mut buffer, &uri, (9..13, "BETA!"), 2);
    assert_eq!(buffer.text().to_string(), v2);
    let at2 = buffer.version();
    let got = next(&s.events);
    assert_eq!(got.version, at2);
    assert_eq!(
        got.words(v2),
        pairs(&[
            ("alpha", "pushed"),
            ("BETA!", "pushed"),
            ("gamma", "pulled")
        ]),
    );

    // A later push keeps the pulled list, mapped to its version.
    s.reply(error(-32801), Duration::ZERO);
    s.edit(&mut buffer, &uri, (0..0, "\n"), 3);
    s.settled();
    let v3 = "\nXX alpha BETA! gamma\n";
    let at3 = buffer.version();
    push(3, vec![over(v3, "XX", "pushed again")]);
    let got = next(&s.events);
    assert_eq!(got.version, at3);
    assert_eq!(
        got.words(v3),
        pairs(&[("XX", "pushed again"), ("gamma", "pulled")])
    );

    // A pull reply whose version has left the ring is dropped. The edits
    // made meanwhile are pulled once it's in.
    s.reply(full(vec![over(v3, "XX", "too old")]), DELAY);
    s.edit(&mut buffer, &uri, (0..0, "\n"), 4);
    let pulls = s.wait_pulls(&uri, 5);
    let mut text = buffer.text().to_string();
    for n in 0..RING as i32 {
        s.edit(&mut buffer, &uri, (0..0, "\n"), 5 + n);
        text.insert(0, '\n');
    }
    assert_eq!(buffer.text().to_string(), text);
    s.reply(full(vec![over(&text, "gamma", "fresh")]), Duration::ZERO);
    s.barrier();
    assert_eq!(s.pulls(&uri).len(), pulls.len());
    s.release();
    let got = next(&s.events);
    assert_eq!(got.version, buffer.version());
    // The push at version 3 has left the ring too.
    assert_eq!(got.words(&text), pairs(&[("gamma", "fresh")]));

    // A reply for an incarnation that was closed is dropped...
    s.reply(full(vec![over(&text, "gamma", "closed")]), DELAY);
    s.edit(&mut buffer, &uri, (0..0, "\n"), 5 + RING as i32);
    s.wait_pulls(&uri, pulls.len() + 2);
    let closed = s.identity(&buffer);
    s.manager.close(buffer.id());
    let cleared = s.events.until("cleared", |event| match event {
        LspEvent::DiagnosticsCleared {
            session,
            incarnation,
            ..
        } => Some((session, incarnation)),
        _ => None,
    });
    assert_eq!(cleared, closed);
    s.release();
    s.settled();
    s.events.assert_quiet();

    // ... and so is one for an incarnation since reopened, which gets its
    // own.
    let mut buffer = Buffer::new(v0);
    s.reply(full(vec![over(v0, "alpha", "first")]), DELAY);
    let uri = s.open(&mut buffer, "b.rs");
    s.wait_pulls(&uri, 1);
    let first = s.identity(&buffer);
    s.manager.close(buffer.id());
    s.reply(full(vec![over(v0, "beta", "reopened")]), Duration::ZERO);
    let uri = s.open(&mut buffer, "b.rs");
    let got = next(&s.events);
    assert_ne!(got.identity, first, "{got:?}");
    assert_eq!(got.identity, s.identity(&buffer));
    assert_eq!(got.words(v0), pairs(&[("beta", "reopened")]));
    s.release();
    s.settled();
    assert_eq!(s.pulls(&uri).len(), 2);
    s.events.assert_quiet();

    // ... and so is one for a session since restarted: its reply never
    // reaches the session after it, whose own pull is answered.
    s.reply(full(vec![over(v0, "alpha", "crashed")]), DELAY);
    // The reopen's didOpen went on from version 0, as version 1.
    s.edit(&mut buffer, &uri, (0..0, "x"), 2);
    s.wait_pulls(&uri, 3);
    let v1 = "xalpha beta gamma\n";
    let before = s.identity(&buffer);
    s.reply(full(vec![over(v1, "beta", "restarted")]), Duration::ZERO);
    s.fake.crash();
    assert_eq!(s.events.status(s.server), ServerStatus::Restarting(1));
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    assert_eq!(s.events.status(s.server), ServerStatus::Starting);
    let got = next(&s.events);
    assert_ne!(got.identity.0, before.0);
    assert_eq!(got.identity, s.identity(&buffer));
    assert_eq!(got.words(v1), pairs(&[("beta", "restarted")]));
    assert_eq!(pulls_in(&s.fake.records_of(2), &uri).len(), 1);
    s.barrier();
    s.events.assert_quiet();

    // ... and so is one landing after the manager closed and reopened the
    // buffer, retiring its identity, but before the actor heard of either.
    // The server thread is held on a write meanwhile; once it goes on, the
    // actor takes the reply before the close and the open waiting behind.
    let mut c = Buffer::new(v0);
    s.reply(full(vec![over(v0, "alpha", "raced")]), DELAY);
    let uc = s.open(&mut c, "c.rs");
    s.wait_pulls(&uc, 1);
    let raced = s.identity(&c);
    let raced_id = (pulls_ids(&s.fake.records(), &uc))
        .pop()
        .expect("the raced pull's request");
    s.fake.set_writes(Writes::Stepped);
    let echo = s.manager.request::<Echo>(s.server, json!({}));
    wait_until("the server thread held on a write", || {
        s.fake.write_waiting()
    });
    s.manager.close(c.id());
    s.reply(
        full(vec![over(v0, "beta", "after the race")]),
        Duration::ZERO,
    );
    let uc = s.open_unsynced(&mut c, "c.rs");
    assert_eq!(s.manager.active_identity(c.id(), s.server), None);
    // A publication in the same interval is dropped too.
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uc.clone(),
            diagnostics: vec![over(v0, "alpha", "pushed in the race")],
            version: Some(0),
        });
    s.release();
    s.fake.wait_for("the raced pull answered", |records| {
        (records.iter()).any(|r| matches!(r, Record::Replied { id, .. } if *id == raced_id))
            && pulls_in(records, &uc).len() == 1
    });
    s.fake.set_writes(Writes::Open);
    answer(echo).unwrap();
    let got = next(&s.events);
    assert_ne!(got.identity, raced, "{got:?}");
    assert_eq!(got.identity, s.identity(&c));
    assert_eq!(got.words(v0), pairs(&[("beta", "after the race")]));

    older_pull_replies_are_dropped();
}

/// A pull reply older than one applied is dropped, and each list keeps its
/// own newest version. With one pull in flight at a time, replies come in
/// order, so this is checked on the lists.
fn older_pull_replies_are_dropped() {
    let ring: VecDeque<SentVersion> = [("ab\n", vec![]), ("xab\n", vec![(0..0, 0..1)])]
        .into_iter()
        .enumerate()
        .map(|(n, (text, edits))| SentVersion {
            lsp_version: n as i32,
            version: Version::default(),
            text: Text::new(text),
            edits: vec![
                edits
                    .into_iter()
                    .map(|(old, new)| Edit { old, new })
                    .collect::<Vec<_>>()
                    .into(),
            ],
            edits_complete: true,
        })
        .collect();
    let uri = Url::parse("file:///a").unwrap();
    let over_b = |text| vec![over(text, "b", "b")];
    let mut lists = Lists::default();
    let accept = |lists: &mut Lists, source, version, items| {
        lists
            .accept(source, version, items, &ring, PositionEncoding::Utf8, &uri)
            .map(|(_, items)| {
                items
                    .into_iter()
                    .map(|item| (item.range.start, item.range.end))
                    .collect::<Vec<_>>()
            })
    };
    assert_eq!(
        accept(&mut lists, Source::Pulled, 1, over_b("xab\n")),
        Some(vec![(2, 3)])
    );
    assert_eq!(accept(&mut lists, Source::Pulled, 0, over_b("ab\n")), None);
    // A push for version 0 is the push's own first, mapped to version 1.
    assert_eq!(
        accept(&mut lists, Source::Pushed, 0, over_b("ab\n")),
        Some(vec![(2, 3), (2, 3)])
    );
}

#[test]
fn refresh_ready_and_errors() {
    let mut s = Pulling::new(true);
    let text = "alpha beta\n";
    let mut a = Buffer::new(text);
    let mut b = Buffer::new(text);
    let ua = s.open(&mut a, "a.rs");
    let ub = s.open(&mut b, "b.rs");
    let (ida, idb) = (a.id(), b.id());
    // Each is pulled once open, with nothing found.
    for _ in 0..2 {
        assert!(next(&s.events).items.is_empty());
    }

    // A refresh is answered, and every open document pulled again.
    let refresh = |s: &Pulling| {
        assert_eq!(
            s.fake.request::<WorkspaceDiagnosticRefresh>(()),
            Ok(Value::Null)
        );
    };
    let both = |s: &Pulling, message: &str| {
        let mut got: Vec<_> = (0..2)
            .map(|_| {
                let got = next(&s.events);
                (got.buffer, got.words(text))
            })
            .collect();
        got.sort_by_key(|(buffer, _)| *buffer != ida);
        assert_eq!(
            got,
            [
                (ida, pairs(&[("alpha", message)])),
                (idb, pairs(&[("alpha", message)]))
            ],
        );
    };
    s.reply(full(vec![over(text, "alpha", "refreshed")]), Duration::ZERO);
    refresh(&s);
    s.wait_pulls(&ua, 2);
    s.wait_pulls(&ub, 2);
    both(&s, "refreshed");

    // An error is dropped; the next change pulls again.
    s.reply(error(-32802), Duration::ZERO);
    s.edit(&mut a, &ua, (11..11, " "), 1);
    s.wait_pulls(&ua, 3);
    s.settled();
    s.events.assert_quiet();
    s.reply(full(vec![over(text, "alpha", "changed")]), Duration::ZERO);
    s.edit(&mut a, &ua, (12..12, " "), 2);
    let got = next(&s.events);
    assert_eq!((got.buffer, got.version), (a.id(), a.version()));
    assert_eq!(got.words(text), pairs(&[("alpha", "changed")]));

    // So is an unchanged report; the next save pulls again.
    s.reply(unchanged(), Duration::ZERO);
    s.edit(&mut a, &ua, (13..13, " "), 3);
    s.wait_pulls(&ua, 5);
    s.settled();
    s.events.assert_quiet();
    s.reply(full(vec![over(text, "alpha", "saved")]), Duration::ZERO);
    s.save(&mut a, &ua, 1);
    let got = next(&s.events);
    assert_eq!((got.buffer, got.version), (a.id(), a.version()));
    assert_eq!(got.words(text), pairs(&[("alpha", "saved")]));

    // Errors on a refresh are dropped; turning Ready pulls again.
    s.reply(error(-32801), Duration::ZERO);
    refresh(&s);
    s.wait_pulls(&ua, 7);
    s.wait_pulls(&ub, 3);
    s.settled();
    s.events.assert_quiet();
    s.reply(full(vec![over(text, "alpha", "ready")]), Duration::ZERO);
    s.clock.wait_for_sleep(crate::readiness::SETTLE);
    s.clock.advance(crate::readiness::SETTLE);
    assert_eq!(s.events.status(s.server), ServerStatus::Ready);
    both(&s, "ready");

    // A timeout too; the next refresh pulls again.
    s.reply(Reply::Never, Duration::ZERO);
    s.edit(&mut b, &ub, (11..11, " "), 1);
    s.wait_pulls(&ub, 5);
    s.barrier();
    s.clock.wait_for_sleep(crate::pull::PULL_TIMEOUT);
    s.clock.advance(crate::pull::PULL_TIMEOUT);
    s.fake.wait_for("the timed-out pull cancelled", |records| {
        records.iter().any(|r| r.is_notification("$/cancelRequest"))
    });
    s.barrier();
    s.events.assert_quiet();
    s.reply(
        full(vec![over(text, "alpha", "refreshed again")]),
        Duration::ZERO,
    );
    refresh(&s);
    both(&s, "refreshed again");
}
