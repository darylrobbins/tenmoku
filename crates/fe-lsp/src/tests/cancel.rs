//! Cancellation, timeouts and the orphan budget (§3.5), what the fake
//! records of the client's answers, and queued commands under steady
//! drains.

mod own_timeouts;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use fe_config::settings::{LspSettings, LspTimeouts};
use futures::FutureExt;
use lsp_types::request::{
    GotoDefinition, HoverRequest, RegisterCapability, Request, WorkspaceConfiguration,
};
use lsp_types::{
    ConfigurationItem, ConfigurationParams, GotoDefinitionParams, HoverParams, NumberOrString,
    Registration, RegistrationParams, Url,
};
use serde_json::{Value, json};

use super::readiness::{answer_definition, definition_params, location};
use super::{Clocked, ROOT, answer, manager, wait_until};
use crate::fake::{
    Echo, FakeServer, InitializeReply, OnExit, Record, Reply, Script, Scripted, Writes,
};
use crate::server::{BACKSTOP, QUEUED_MAX};
use crate::{LspError, ManualClock, ServerSpec, ServerStatus};

/// The ids of the `method` requests the fake got, in order.
pub(super) fn requests_of(records: &[Record], method: &str) -> Vec<NumberOrString> {
    records
        .iter()
        .filter_map(|record| match record {
            Record::Request { id, method: m, .. } if m == method => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Waits for the fake to get `$/cancelRequest` for `id`.
fn wait_cancelled(fake: &FakeServer, id: &NumberOrString) {
    fake.wait_for("$/cancelRequest", |records| {
        records.iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == json!(id))
        })
    });
}

macro_rules! fake_request {
    ($name:ident, $method:literal) => {
        enum $name {}
        impl Request for $name {
            type Params = Value;
            type Result = Value;
            const METHOD: &'static str = $method;
        }
    };
}

fake_request!(RequestCancelled, "fake/request-cancelled");
fake_request!(ContentModified, "fake/content-modified");
fake_request!(ServerCancelled, "fake/server-cancelled");

fn hover_params() -> HoverParams {
    HoverParams {
        text_document_position_params: definition_params().text_document_position_params,
        work_done_progress_params: Default::default(),
    }
}

#[test]
fn drop_and_timeout_cancel() {
    let fake_clock = Arc::new(ManualClock::new());
    let late = |value: Value| Scripted {
        reply: Reply::Result(value),
        delay: Duration::from_secs(60),
    };
    let error = |code: i32| {
        Scripted::now(Reply::Error {
            code,
            message: "no".into(),
        })
    };
    let fake = FakeServer::with_clock(
        Script {
            replies: HashMap::from([
                ("textDocument/definition".to_owned(), late(location())),
                ("textDocument/hover".to_owned(), late(Value::Null)),
                (RequestCancelled::METHOD.to_owned(), error(-32800)),
                (ContentModified::METHOD.to_owned(), error(-32801)),
                (ServerCancelled::METHOD.to_owned(), error(-32802)),
            ]),
            ..Script::default()
        },
        fake_clock.clone(),
    );
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            std::path::Path::new(ROOT),
        )
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();

    // Dropped once sent: `$/cancelRequest` with the id the fake got.
    let dropped = manager.request::<GotoDefinition>(server, definition_params());
    let records = fake.wait_for("a definition", |records| {
        requests_of(records, GotoDefinition::METHOD).len() == 1
    });
    let first = requests_of(&records, GotoDefinition::METHOD)[0].clone();
    drop(dropped);
    wait_cancelled(&fake, &first);

    // Past `lsp.timeouts`' 5 s for a definition: `Timeout` at once, and
    // cancelled.
    let mut timed = manager
        .request::<GotoDefinition>(server, definition_params())
        .boxed();
    let records = fake.wait_for("a second definition", |records| {
        requests_of(records, GotoDefinition::METHOD).len() == 2
    });
    let second = requests_of(&records, GotoDefinition::METHOD)[1].clone();
    clock.wait_for_sleep(Duration::from_secs(5));
    clock.advance(Duration::from_millis(4999));
    std::thread::sleep(Duration::from_millis(50));
    assert!(timed.as_mut().now_or_never().is_none());
    clock.advance(Duration::from_millis(1));
    assert_eq!(answer(timed), Err(LspError::Timeout));
    wait_cancelled(&fake, &second);

    // Another method's timeout, as set.
    manager.update_settings(&LspSettings {
        timeouts: LspTimeouts {
            hover: 1234,
            ..LspTimeouts::default()
        },
        ..LspSettings::default()
    });
    let hover = manager.request::<HoverRequest>(server, hover_params());
    let records = fake.wait_for("a hover", |records| {
        requests_of(records, HoverRequest::METHOD).len() == 1
    });
    clock.wait_for_sleep(Duration::from_millis(1234));
    clock.advance(Duration::from_millis(1234));
    assert_eq!(answer(hover), Err(LspError::Timeout));
    wait_cancelled(&fake, &requests_of(&records, HoverRequest::METHOD)[0]);

    // The late replies reach no caller: the orphans take them.
    wait_until("3 orphans", || manager.orphans(server).0 == 3);
    fake_clock.advance(Duration::from_secs(60));
    fake.wait_for("the late replies", |records| {
        records
            .iter()
            .filter(|record| matches!(record, Record::Replied { .. }))
            .count()
            >= 4
    });
    wait_until("the orphans to go", || manager.orphans(server).0 == 0);

    // A request still waiting to be sent (behind `initialize`) times out
    // from the call too, and is never sent.
    let slow = FakeServer::new(Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    });
    let slow_clock = Arc::new(ManualClock::new());
    let (mut slow_manager, slow_events) = super::manager(slow.launcher(), Some(slow_clock.clone()));
    let starting = slow_manager
        .start(
            ServerSpec::new("slow", "fake-server"),
            std::path::Path::new(ROOT),
        )
        .unwrap();
    assert_eq!(slow_events.status(starting), ServerStatus::Starting);
    let queued = slow_manager.request::<GotoDefinition>(starting, definition_params());
    slow_clock.wait_for_sleep(Duration::from_secs(5));
    slow_clock.advance(Duration::from_secs(5));
    assert_eq!(answer(queued), Err(LspError::Timeout));
    assert!(requests_of(&slow.records(), GotoDefinition::METHOD).is_empty());

    // Error codes.
    let reply = |reply: Result<crate::Reply<Value>, LspError>| reply.map(|r| r.result);
    assert_eq!(
        reply(answer(
            manager.request::<RequestCancelled>(server, json!({}))
        )),
        Err(LspError::Cancelled)
    );
    assert_eq!(
        reply(answer(
            manager.request::<ContentModified>(server, json!({}))
        )),
        Err(LspError::Retry)
    );
    assert_eq!(
        reply(answer(
            manager.request::<ServerCancelled>(server, json!({}))
        )),
        Err(LspError::Retry)
    );
}

/// `n` definition requests, once the fake has got them all.
pub(super) fn sent_definitions(
    s: &Clocked,
    launch: usize,
    n: usize,
) -> Vec<
    impl Future<Output = Result<crate::Reply<Option<lsp_types::GotoDefinitionResponse>>, LspError>>,
> {
    let before = requests_of(&s.fake.records_of(launch), GotoDefinition::METHOD).len();
    let requests: Vec<_> = (0..n)
        .map(|_| {
            s.manager
                .request::<GotoDefinition>(s.server, definition_params())
        })
        .collect();
    s.fake.wait_for_at(launch, "the definitions", |records| {
        requests_of(records, GotoDefinition::METHOD).len() == before + n
    });
    requests
}

#[test]
fn orphan_budget() {
    let never = HashMap::from([(
        "textDocument/definition".to_owned(),
        Scripted::now(Reply::Never),
    )]);

    // A server that answers cancelled requests: the orphans drain.
    let s = Clocked::new(
        ServerSpec::new("fake", "fake-server"),
        Script {
            replies: never.clone(),
            answer_cancels: true,
            ..Script::default()
        },
    );
    drop(sent_definitions(&s, 1, 10));
    s.fake.wait_for("10 answered cancels", |records| {
        records
            .iter()
            .filter(|record| matches!(record, Record::Replied { method, .. } if method == GotoDefinition::METHOD))
            .count()
            == 10
    });
    wait_until("the orphans to go", || s.manager.orphans(s.server).0 == 0);
    assert!(s.manager.orphans(s.server).1 > 0);

    // Orphans whose replies came but weren't let go yet don't count.
    let mut orphans = crate::requests::Orphans::default();
    let replies: Vec<_> = (0..crate::ORPHANS_MAX)
        .map(|_| {
            let (reply, replied) = futures::channel::oneshot::channel::<()>();
            assert!(orphans.adopt(replied.map(drop).boxed()));
            reply
        })
        .collect();
    for reply in replies {
        reply.send(()).unwrap();
    }
    assert!(orphans.adopt(futures::future::pending().boxed()));
    assert_eq!(orphans.len(), 1);

    // A server that never answers them: the 65th abandoned request
    // recycles the session (shutdown, kill, a new initialize), as a crash.
    let s = Clocked::new(
        ServerSpec::new("fake", "fake-server"),
        Script {
            replies: never,
            answer_shutdown: false,
            on_exit: OnExit::Ignore,
            ..Script::default()
        },
    );
    for round in 1..=3u32 {
        let launch = round as usize;
        drop(sent_definitions(&s, launch, 64));
        wait_until("64 orphans", || s.manager.orphans(s.server) == (64, 64));
        assert!(
            s.fake
                .records_of(launch)
                .iter()
                .all(|r| r.method() != "shutdown")
        );
        drop(sent_definitions(&s, launch, 1));
        s.fake.wait_for_at(launch, "shutdown", |records| {
            records.iter().any(|r| r.method() == "shutdown")
        });
        s.clock.wait_for_sleep(Duration::from_secs(2));
        s.clock.advance(Duration::from_secs(2));
        s.clock.wait_for_sleep(Duration::from_secs(1));
        s.clock.advance(Duration::from_secs(1));
        loop {
            match s.events.status(s.server) {
                ServerStatus::Ready | ServerStatus::Starting => {}
                status => {
                    assert_eq!(status, ServerStatus::Restarting(round));
                    break;
                }
            }
        }
        assert_eq!(s.fake.kills(), launch);
        // It never held more than 64.
        assert_eq!(s.manager.orphans(s.server), (64, 64));
        let backoff = Duration::from_secs(1 << (round - 1));
        s.clock.wait_for_sleep(backoff);
        s.clock.advance(backoff);
        s.expect(ServerStatus::Starting);
        s.fake.wait_initialized_at(launch + 1);
        assert_eq!(s.manager.orphans(s.server), (0, 0));
    }
}

#[test]
fn fake_records_client_responses() {
    let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    let configuration = |section: &str| ConfigurationParams {
        items: vec![ConfigurationItem {
            scope_uri: None,
            section: Some(section.into()),
        }],
    };
    // A result, and an error.
    assert_eq!(
        s.fake
            .request::<WorkspaceConfiguration>(configuration("a"))
            .unwrap(),
        json!([null])
    );
    let refused = s
        .fake
        .request::<RegisterCapability>(RegistrationParams {
            registrations: vec![Registration {
                id: "w".into(),
                method: "workspace/didChangeWatchedFiles".into(),
                register_options: None,
            }],
        })
        .unwrap_err();
    assert_eq!(refused.code, Some(-32803));
    // A new launch numbers its requests afresh.
    s.fake.crash();
    s.expect(ServerStatus::Restarting(1));
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    s.fake.wait_initialized_at(2);
    s.fake
        .request::<WorkspaceConfiguration>(configuration("b"))
        .unwrap();

    let responses: Vec<(usize, Record)> = s
        .fake
        .timeline()
        .into_iter()
        .filter(|(_, record)| matches!(record, Record::Response { .. }))
        .collect();
    assert_eq!(
        responses,
        [
            (
                1,
                Record::Response {
                    id: NumberOrString::Number(0),
                    result: Ok(json!([null])),
                }
            ),
            (
                1,
                Record::Response {
                    id: NumberOrString::Number(1),
                    result: Err((-32803, refused.message)),
                }
            ),
            (
                2,
                Record::Response {
                    id: NumberOrString::Number(0),
                    result: Ok(json!([null])),
                }
            ),
        ]
    );
}

#[test]
fn drains_do_not_starve_queued_commands() {
    let mut s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    answer_definition(&s.fake, location());
    // The definition waits longer than a definition's timeout.
    s.manager.update_settings(&LspSettings {
        timeouts: LspTimeouts {
            definition: 60_000,
            ..LspTimeouts::default()
        },
        ..LspSettings::default()
    });
    s.settle();
    let mut buffers: Vec<(Buffer, Url)> = ["a.txt", "b.txt", "c.txt"]
        .into_iter()
        .map(|name| {
            let mut buffer = Buffer::from_file_contents("text\n");
            let path = PathBuf::from(ROOT).join(name);
            s.manager
                .open_on(
                    s.server,
                    buffer.id(),
                    buffer.text().clone(),
                    buffer.version(),
                    buffer.subscribe(),
                    &path,
                    "text",
                )
                .unwrap();
            let uri = Url::from_file_path(&path).unwrap();
            s.fake
                .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
            (buffer, uri)
        })
        .collect();

    // async-lsp's queue full (the window's worth of echoes handed to it,
    // one more at most waiting), then a definition and a close.
    s.manager.record_handed(s.server);
    s.fake.set_writes(Writes::Held);
    let echoes: Vec<_> = (0..=QUEUED_MAX)
        .map(|n| s.manager.request::<Echo>(s.server, json!({ "n": n })))
        .collect();
    let handed_echoes = |s: &Clocked| {
        s.manager
            .handed(s.server)
            .iter()
            .filter(|what| *what == Echo::METHOD)
            .count()
    };
    wait_until("the window full", || {
        handed_echoes(&s) >= QUEUED_MAX as usize
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(handed_echoes(&s) <= QUEUED_MAX as usize + 1);
    let (b, c) = (&buffers[1].0, &buffers[2].0);
    let definition =
        s.manager
            .document_request::<GotoDefinition>(b.id(), b.version(), |admitted| {
                GotoDefinitionParams {
                    text_document_position_params: lsp_types::TextDocumentPositionParams {
                        text_document: lsp_types::TextDocumentIdentifier {
                            uri: admitted.document.uri.clone(),
                        },
                        position: lsp_types::Position::new(0, 0),
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }
            });
    s.manager.close(c.id());
    let closed = format!("didClose {}", buffers[2].1);
    let changed = format!("didChange {}", buffers[0].1);

    // Edits and ticks throughout, with a writer freeing one slot per tick:
    // async-lsp writes a message as its header, then its body.
    s.fake.set_writes(Writes::Paced);
    let a = &mut buffers[0].0;
    let interval = |a: &mut Buffer| {
        a.edit([(0..0, "x")], Origin::User).unwrap();
        s.manager.changed(a.id());
        s.clock.wait_for_sleep(BACKSTOP);
        s.clock.advance(BACKSTOP);
        s.fake.allow_write();
        s.fake.allow_write();
        s.fake.wait_paced();
    };
    for _ in 0..3 {
        interval(a);
    }
    // Both handed to async-lsp within 3 intervals, writes still paced.
    wait_until("the definition and the close handed", || {
        let handed = s.manager.handed(s.server);
        handed.iter().any(|what| what == GotoDefinition::METHOD) && handed.contains(&closed)
    });
    let handed = s.manager.handed(s.server);
    let at = |what: &str| handed.iter().position(|h| h == what).unwrap();
    let drains_ahead = |of: &str| handed[..at(of)].iter().filter(|h| **h == changed).count();
    assert!(drains_ahead(GotoDefinition::METHOD) <= 3, "{handed:?}");
    assert!(drains_ahead(&closed) <= 3, "{handed:?}");
    // The edits go on being drained.
    for _ in 0..5 {
        interval(a);
    }
    s.fake.set_writes(Writes::Open);
    answer(definition).unwrap();
    for echo in echoes {
        answer(echo).unwrap();
    }
    // The last edits drained, and on the wire.
    s.manager.changed(buffers[0].0.id());
    answer(s.manager.request::<Echo>(s.server, json!({}))).unwrap();

    // The order they were sent in: at most 3 drains got ahead of each.
    let a_uri = json!(buffers[0].1);
    let c_uri = json!(buffers[2].1);
    let mut changes = 0;
    let mut ahead = HashMap::new();
    for record in s.fake.records() {
        match record {
            Record::Notification { method, params }
                if method == "textDocument/didChange" && params["textDocument"]["uri"] == a_uri =>
            {
                changes += 1;
            }
            Record::Notification { method, params }
                if method == "textDocument/didClose" && params["textDocument"]["uri"] == c_uri =>
            {
                ahead.insert("didClose", changes);
            }
            Record::Request { method, .. } if method == GotoDefinition::METHOD => {
                ahead.insert("definition", changes);
            }
            _ => {}
        }
    }
    // On the wire, in the order handed.
    assert!(changes >= 1, "{changes} didChanges");
    assert!(ahead["definition"] <= 3, "{ahead:?} of {changes}");
    assert!(ahead["didClose"] <= 3, "{ahead:?} of {changes}");
}

#[test]
fn a_command_flood_keeps_ticks_and_stop_responsive() {
    let fake = FakeServer::new(Script::default());
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            std::path::Path::new(ROOT),
        )
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    let mut buffer = Buffer::from_file_contents("text\n");
    let path = PathBuf::from(ROOT).join("a.txt");
    manager
        .open_on(
            server,
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &path,
            "text",
        )
        .unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    buffer.edit([(0..0, "x")], Origin::User).unwrap();
    let manager = Arc::new(std::sync::Mutex::new(manager));
    let flooding = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flood = std::thread::spawn({
        let (manager, flooding) = (manager.clone(), flooding.clone());
        let id = buffer.id();
        move || {
            let mut kept = Vec::new();
            while flooding.load(std::sync::atomic::Ordering::SeqCst) {
                let manager = manager.lock().unwrap();
                manager.changed(id);
                kept.push(manager.request::<Echo>(server, json!({})));
            }
        }
    });
    // Ticks keep coming round, each queueing a drain behind the flood.
    for _ in 0..5 {
        clock.wait_for_sleep(BACKSTOP);
        clock.advance(BACKSTOP);
    }
    manager.lock().unwrap().stop(server);
    loop {
        if let ServerStatus::Stopped(why) = events.status(server) {
            assert_eq!(why, None);
            break;
        }
    }
    flooding.store(false, std::sync::atomic::Ordering::SeqCst);
    flood.join().unwrap();
}
