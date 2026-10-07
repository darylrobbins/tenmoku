//! Crashes, the rolling restart budget, and restarts as new sessions
//! (§3.5), on the injected clock.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use lsp_types::Url;
use serde_json::json;

use super::{Clocked, Events, ROOT, answer, capture_logs, logs_from, manager};
use crate::fake::{Echo, FakeServer, InitializeReply, Record, Script};
use crate::{LspError, LspManager, ManualClock, ServerId, ServerSpec, ServerStatus};

/// The next status of `server` that isn't about readiness.
fn next_crash_status(events: &Events, server: ServerId) -> ServerStatus {
    loop {
        match events.status(server) {
            ServerStatus::Starting | ServerStatus::Ready | ServerStatus::Indexing { .. } => {}
            status => return status,
        }
    }
}

/// Waits out a backoff of `secs`: nothing relaunches a moment before it
/// ends, and it starts again when it does.
fn backoff(fake: &FakeServer, clock: &ManualClock, events: &Events, server: ServerId, secs: u64) {
    let launches = fake.launches();
    let backoff = Duration::from_secs(secs);
    clock.wait_for_sleep(backoff);
    clock.advance(backoff - Duration::from_millis(1));
    events.assert_quiet();
    assert_eq!(fake.launches(), launches);
    clock.advance(Duration::from_millis(1));
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized_at(launches + 1);
}

struct Crashing {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    server: ServerId,
    events: Events,
}

impl Crashing {
    fn new() -> Self {
        let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
        Self {
            fake: s.fake,
            clock: s.clock,
            manager: s.manager,
            server: s.server,
            events: s.events,
        }
    }

    fn crash(&self) -> ServerStatus {
        self.fake.crash();
        next_crash_status(&self.events, self.server)
    }

    fn backoff(&self, secs: u64) {
        backoff(&self.fake, &self.clock, &self.events, self.server, secs);
    }
}

#[test]
fn rolling_window_backoff() {
    // Crashes at 0, 1, 3, 7, 15 and 31 s: restarts 1, 2, 4, 8 and 16 s
    // later, then stopped.
    let s = Crashing::new();
    for n in 1..=5 {
        assert_eq!(s.crash(), ServerStatus::Restarting(n));
        s.backoff(1 << (n - 1));
    }
    let ServerStatus::Stopped(Some(why)) = s.crash() else {
        panic!("not stopped at the 6th crash");
    };
    assert!(why.contains("fake"), "{why}");
    super::wait_until("the thread to end", || s.manager.thread_finished(s.server));
    // Stopped until a restart.
    s.clock.advance(Duration::from_secs(600));
    assert_eq!(s.fake.launches(), 6);

    // A restart clears the list and starts it from Stopped.
    let mut manager = s.manager;
    let again = manager.restart("fake", Path::new(ROOT)).unwrap().unwrap();
    assert_ne!(again, s.server);
    assert_eq!(s.events.status(again), ServerStatus::Starting);
    s.fake.wait_initialized_at(7);
    s.fake.crash();
    assert_eq!(
        next_crash_status(&s.events, again),
        ServerStatus::Restarting(1)
    );

    // A crash every 2 minutes: the window holds at most 2.
    let s = Crashing::new();
    for round in 0..8 {
        let status = s.crash();
        let n = if round == 0 { 1 } else { 2 };
        assert_eq!(status, ServerStatus::Restarting(n), "round {round}");
        s.backoff(n.into());
        s.clock
            .advance(Duration::from_secs(120) - Duration::from_secs(n.into()));
    }

    // Crashes just over 3 minutes apart: n stays 1.
    let s = Crashing::new();
    for round in 0..8 {
        assert_eq!(s.crash(), ServerStatus::Restarting(1), "round {round}");
        s.backoff(1);
        s.clock
            .advance(Duration::from_secs(180) - Duration::from_secs(1) + Duration::from_millis(1));
    }
}

#[test]
fn restart_is_a_new_session() {
    let mut s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    let mut a = Buffer::from_file_contents("one\n");
    let mut b = Buffer::from_file_contents("two\n");
    let uris: Vec<Url> = [(&mut a, "a.txt"), (&mut b, "b.txt")]
        .into_iter()
        .map(|(buffer, name)| {
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
            Url::from_file_path(&path).unwrap()
        })
        .collect();
    for uri in &uris {
        s.fake
            .wait_for_document(uri, "didOpen", |r| r.is_some_and(|r| r.open));
    }
    a.edit([(0..0, "1")], Origin::User).unwrap();
    s.manager.changed(a.id());
    s.fake
        .wait_for_document(&uris[0], "version 1", |r| r.is_some_and(|r| r.version == 1));
    let old = s.manager.active_identity(a.id(), s.server).unwrap();
    let pending = s
        .manager
        .request::<Echo>(s.server, json!({ "never": true }));
    s.fake.wait_for("the pending request", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { params, .. } if params["never"] == true))
    });

    // The fake closes its transport.
    s.fake.crash();
    assert_eq!(s.events.status(s.server), ServerStatus::Restarting(1));
    assert_eq!(answer(pending), Err(LspError::ServerDown));
    // The old session's identities were retired.
    assert_eq!(s.manager.active_identity(a.id(), s.server), None);
    assert_eq!(s.manager.active_identity(b.id(), s.server), None);
    // Edited while it waits to restart.
    b.edit([(0..0, "2")], Origin::User).unwrap();
    s.manager.changed(b.id());
    assert_eq!(
        answer(s.manager.request::<Echo>(s.server, json!({}))),
        Err(LspError::ServerDown)
    );

    backoff(&s.fake, &s.clock, &s.events, s.server, 1);
    let init = |launch| {
        s.fake
            .records_of(launch)
            .into_iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == "initialize"))
            .count()
    };
    assert_eq!((init(1), init(2)), (1, 1));
    for (uri, text) in uris.iter().zip(["1one\n", "2two\n"]) {
        let replica = s
            .fake
            .wait_for_document(uri, "reopened", |r| r.is_some_and(|r| r.launch == 2))
            .unwrap();
        assert_eq!((replica.text.as_str(), replica.version), (text, 0));
    }
    let new = s.manager.active_identity(a.id(), s.server).unwrap();
    assert_ne!(new.0, old.0, "a new session id");
    assert!(new.1 != old.1);
    // Readiness starts again.
    s.settle();
    // Edits go on from version 0.
    a.edit([(0..0, "x")], Origin::User).unwrap();
    s.manager.changed(a.id());
    let replica = s
        .fake
        .wait_for_document(&uris[0], "version 1", |r| {
            r.is_some_and(|r| r.launch == 2 && r.version == 1)
        })
        .unwrap();
    assert_eq!(replica.text, "x1one\n");
    assert_eq!(s.fake.sync_errors(), Vec::<String>::new());

    // An initialize failure and an initialize timeout are crashes.
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Error {
            code: -32603,
            message: "no".into(),
        },
        ..Script::default()
    });
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let id = manager
        .start(ServerSpec::new("fails", "fake-server"), Path::new(ROOT))
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    assert_eq!(events.status(id), ServerStatus::Restarting(1));
    clock.wait_for_sleep(Duration::from_secs(1));
    clock.advance(Duration::from_secs(1));
    assert_eq!(events.status(id), ServerStatus::Starting);
    assert_eq!(events.status(id), ServerStatus::Restarting(2));

    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    });
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    let id = manager
        .start(ServerSpec::new("silent", "fake-server"), Path::new(ROOT))
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    clock.wait_for_sleep(Duration::from_secs(30));
    clock.advance(Duration::from_secs(30));
    assert_eq!(events.status(id), ServerStatus::Restarting(1));
    assert_eq!(fake.kills(), 1);
}

#[test]
fn exit_with_transport_open() {
    // During initialize: the exit ends the session, though its output
    // stays open and the clock doesn't move.
    capture_logs("fe-lsp exits-starting");
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    });
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let id = manager
        .start(
            ServerSpec::new("exits-starting", "fake-server"),
            Path::new(ROOT),
        )
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    fake.wait_for("initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    let queued = manager.request::<Echo>(id, json!({}));
    fake.exit_keeping_output(3);
    assert_eq!(events.status(id), ServerStatus::Restarting(1));
    assert_eq!(answer(queued), Err(LspError::ServerDown));
    let logs = logs_from("fe-lsp exits-starting");
    assert!(
        logs.iter()
            .any(|line| line.contains("exited with code 3 before answering initialize")),
        "{logs:?}"
    );

    // After Ready.
    capture_logs("fe-lsp exits-ready");
    let s = Clocked::new(
        ServerSpec::new("exits-ready", "fake-server"),
        Script::default(),
    );
    s.settle();
    let pending = s
        .manager
        .request::<Echo>(s.server, json!({ "never": true }));
    s.fake.wait_for("the pending request", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { params, .. } if params["never"] == true))
    });
    s.fake.exit_keeping_output(3);
    assert_eq!(s.events.status(s.server), ServerStatus::Restarting(1));
    assert_eq!(answer(pending), Err(LspError::ServerDown));
    let logs = logs_from("fe-lsp exits-ready");
    assert!(
        logs.iter()
            .any(|line| line.contains("exits-ready exited with code 3; restarting in 1 s")),
        "{logs:?}"
    );
    // Its old process is killed, so its output closes too.
    assert_eq!(s.fake.kills(), 1);
}

#[test]
fn crash_time_is_when_it_was_found() {
    // Crashes at 0, 1, 3, 7 and 15 s; then at 179 s the orphan budget runs
    // out, and the shutdown takes 3 s more: still the 6th crash in 3
    // minutes.
    let s = Clocked::new(
        ServerSpec::new("fake", "fake-server"),
        Script {
            replies: std::collections::HashMap::from([(
                "textDocument/definition".to_owned(),
                crate::fake::Scripted::now(crate::fake::Reply::Never),
            )]),
            answer_shutdown: false,
            on_exit: crate::fake::OnExit::Ignore,
            ..Script::default()
        },
    );
    for n in 1..=5u32 {
        s.fake.crash();
        assert_eq!(
            next_crash_status(&s.events, s.server),
            ServerStatus::Restarting(n)
        );
        backoff(&s.fake, &s.clock, &s.events, s.server, 1 << (n - 1));
    }
    // 31 s now.
    s.clock.advance(Duration::from_secs(148));
    drop(super::cancel::sent_definitions(
        &s,
        6,
        crate::ORPHANS_MAX + 1,
    ));
    s.fake.wait_for_at(6, "shutdown", |records| {
        records.iter().any(|r| r.method() == "shutdown")
    });
    s.clock.wait_for_sleep(Duration::from_secs(2));
    s.clock.advance(Duration::from_secs(2));
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    assert!(matches!(
        next_crash_status(&s.events, s.server),
        ServerStatus::Stopped(Some(_))
    ));
}

#[test]
fn a_server_that_gave_up_stays_stopped() {
    use fe_config::settings::{LspServerSettings, LspSettings};

    let fake = FakeServer::new(Script::default());
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    // Its settings changed while it ran.
    let mut settings = LspSettings::default();
    let open = |manager: &mut LspManager, buffer: &mut Buffer, name: &str| {
        let path = PathBuf::from(ROOT).join(name);
        manager
            .open(
                Some(Path::new(ROOT)),
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                buffer.subscribe(),
                &path,
                "rust",
            )
            .unwrap()
    };
    let mut a = Buffer::new("fn a() {}\n");
    let server = open(&mut manager, &mut a, "a.rs").unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    settings.servers.insert(
        "rust-analyzer".into(),
        LspServerSettings {
            settings: json!({ "x": 1 }).as_object().cloned(),
            ..LspServerSettings::default()
        },
    );
    manager.update_settings(&settings);
    fake.wait_for("the new settings", |records| {
        records
            .iter()
            .any(|r| r.is_notification("workspace/didChangeConfiguration"))
    });
    for n in 1..=5u32 {
        fake.crash();
        assert_eq!(
            next_crash_status(&events, server),
            ServerStatus::Restarting(n)
        );
        backoff(&fake, &clock, &events, server, 1 << (n - 1));
    }
    fake.crash();
    assert!(matches!(
        next_crash_status(&events, server),
        ServerStatus::Stopped(Some(_))
    ));
    super::wait_until("the thread to end", || manager.thread_finished(server));

    // Neither another document nor the same settings start it again.
    let mut b = Buffer::new("fn b() {}\n");
    assert_eq!(open(&mut manager, &mut b, "b.rs"), None);
    manager.update_settings(&settings);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.launches(), 6);

    // A restart does, with both documents.
    let again = manager
        .restart("rust-analyzer", Path::new(ROOT))
        .unwrap()
        .unwrap();
    assert_eq!(events.status(again), ServerStatus::Starting);
    fake.wait_initialized_at(7);
    for name in ["a.rs", "b.rs"] {
        let uri = Url::from_file_path(PathBuf::from(ROOT).join(name)).unwrap();
        fake.wait_for_document(&uri, "reopened", |r| r.is_some_and(|r| r.launch == 7));
    }
}
