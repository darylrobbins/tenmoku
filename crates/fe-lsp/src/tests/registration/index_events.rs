//! Index-change events (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §5,
//! "Asking again"): `LspEvent::Notified` after each watched-files or
//! configuration notification handed to a server, and the poll clock of a
//! spec's `repoll`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::FileChangeType;
use lsp_types::request::RegisterCapability;
use lsp_types::{Registration, RegistrationParams};
use serde_json::{Value, json};

use super::{CONFIGURATION, WATCHED};
use crate::fake::{FakeServer, Record, Script, fake_path};
use crate::tests::{Clocked, ROOT, manager};
use crate::{
    Clock, LspEvent, ManualClock, Notice, ServerId, ServerSpec, ServerStatus, extension_spec,
};

/// Each `Notified` `server` reports from now on, until `last` is seen.
fn notified_until(events: &crate::tests::Events, server: ServerId, last: Notice) -> Vec<Notice> {
    let mut seen = Vec::new();
    events.until("a Notified", |event| match event {
        LspEvent::Notified { server: from, what } if from == server => {
            seen.push(what);
            (what == last).then_some(())
        }
        _ => None,
    });
    seen
}

#[test]
fn notified_after_each_notification() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    let mut buffer = Buffer::new("fn main() {}\n");
    let server = manager
        .open(
            Some(Path::new(ROOT)),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &fake_path("main.rs"),
            "rust",
        )
        .unwrap()
        .unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    let configure = |manager: &mut crate::LspManager, generation: u64| {
        let mut settings = LspSettings::default();
        let value = json!({ "generation": generation });
        settings.servers.insert(
            "rust-analyzer".into(),
            LspServerSettings {
                settings: value.as_object().cloned(),
                ..LspServerSettings::default()
            },
        );
        manager.update_settings(&settings);
    };

    // A settings change: `didChangeConfiguration`, then its `Notified`.
    configure(&mut manager, 1);
    assert_eq!(
        notified_until(&events, server, Notice::Configuration),
        [Notice::Configuration]
    );
    fake.wait_for("the configuration", |records| {
        records
            .iter()
            .any(|record| record.is_notification(CONFIGURATION))
    });

    // A change on disk the server registered for: one `Notified` per
    // notification sent, not per change.
    let options = json!({ "watchers": [{ "globPattern": "**/*.rs" }] });
    let registered = fake.request::<RegisterCapability>(RegistrationParams {
        registrations: vec![Registration {
            id: "w".into(),
            method: WATCHED.into(),
            register_options: Some(options),
        }],
    });
    assert_eq!(registered, Ok(Value::Null));
    manager.files_changed(vec![
        (fake_path("a.rs"), FileChangeType::CHANGED),
        (fake_path("b.rs"), FileChangeType::CREATED),
    ]);
    assert_eq!(
        notified_until(&events, server, Notice::WatchedFiles),
        [Notice::WatchedFiles]
    );

    // A change nothing watches sends nothing, and reports nothing: the next
    // `Notified` is the next settings change's.
    manager.files_changed(vec![(fake_path("README.md"), FileChangeType::CHANGED)]);
    configure(&mut manager, 2);
    assert_eq!(
        notified_until(&events, server, Notice::Configuration),
        [Notice::Configuration]
    );

    // A watched-files notification queued as such counts too, and other
    // notifications report nothing: the next `Notified` is the settings
    // change's.
    manager.notify_sessions("workspace/didCreateFiles", |_, _| {
        Some(json!({ "files": [] }))
    });
    manager.notify_sessions(WATCHED, |_, _| Some(json!({ "changes": [] })));
    configure(&mut manager, 3);
    assert_eq!(
        notified_until(&events, server, Notice::Configuration),
        [Notice::WatchedFiles, Notice::Configuration]
    );

    // Each `Notified` followed a notification the server got, in order.
    let records = fake.wait_for("every notification", |records| {
        (records.iter())
            .filter(|record| {
                record.is_notification(WATCHED) || record.is_notification(CONFIGURATION)
            })
            .count()
            == 5
    });
    let methods: Vec<&str> = (records.iter())
        .filter_map(|record| match record {
            Record::Notification { method, .. } if method == WATCHED || method == CONFIGURATION => {
                Some(method.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        methods,
        [
            CONFIGURATION,
            WATCHED,
            CONFIGURATION,
            WATCHED,
            CONFIGURATION
        ]
    );
}

#[test]
fn repoll_due_on_manual_clock() {
    let period = Duration::from_secs(3);
    let mut s = Clocked::new(
        ServerSpec {
            repoll: Some(period),
            ..ServerSpec::new("fake", "fake-server")
        },
        Script::default(),
    );
    // The clock hasn't moved: the session initialized at `start`.
    let start = s.clock.now();
    s.settle();
    let at = |after: Duration| s.manager.repoll_due(s.server, start + after);

    // Nothing is due before one whole period, nor before the session began.
    assert_eq!(at(Duration::ZERO), None);
    assert_eq!(at(period - Duration::from_millis(1)), None);
    assert_eq!(
        s.manager
            .repoll_due(s.server, start - Duration::from_secs(1)),
        None
    );
    // Then the latest due poll, which stays put until the next period.
    assert_eq!(at(period), Some(start + period));
    assert_eq!(
        at(period * 2 - Duration::from_millis(1)),
        Some(start + period)
    );
    assert_eq!(at(period * 2), Some(start + period * 2));
    assert_eq!(
        at(Duration::from_secs(3600) + Duration::from_millis(1)),
        Some(start + Duration::from_secs(3600))
    );
    // On the manager's clock, as it moves.
    s.clock.advance(Duration::from_secs(7));
    assert_eq!(
        s.manager.repoll_due(s.server, s.clock.now()),
        Some(start + period * 2)
    );

    // A server without `repoll` has none due, and neither has one that has
    // gone.
    let plain = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    let start = plain.clock.now();
    plain.settle();
    assert_eq!(
        plain
            .manager
            .repoll_due(plain.server, start + Duration::from_secs(3600)),
        None
    );
    s.manager.stop(s.server);
    let later = s.clock.now() + period * 5;
    assert_eq!(s.manager.repoll_due(s.server, later), None);
}

#[test]
fn repoll_ms_parsed_and_defaults_off() {
    // Off by default, in the spec and in settings.
    assert_eq!(ServerSpec::new("x", "x").repoll, None);
    assert_eq!(LspServerSettings::default().repoll_ms, None);
    assert!(
        (crate::registry::builtin_specs().iter()).all(|spec| spec.repoll.is_none()),
        "no built-in server polls yet"
    );

    let parsed = |table: Value| -> LspServerSettings { serde_json::from_value(table).unwrap() };
    let spec = |table: Value| extension_spec("x", &parsed(table)).unwrap();
    assert_eq!(spec(json!({ "command": "x" })).repoll, None);
    assert_eq!(
        spec(json!({ "command": "x", "repoll_ms": 3000 })).repoll,
        Some(Duration::from_millis(3000))
    );
    // 0 is off too.
    assert_eq!(spec(json!({ "command": "x", "repoll_ms": 0 })).repoll, None);
    // A negative period isn't one.
    assert!(serde_json::from_value::<LspServerSettings>(json!({ "repoll_ms": -1 })).is_err());

    // The watch keys are parsed and applied the same way: a list set in
    // settings replaces a pointer, unless a pointer is set there too.
    let base = ServerSpec {
        watch_files_from: Some("/a/files".into()),
        watch_excludes_from: Some("/a/excludes".into()),
        ..ServerSpec::new("x", "x")
    };
    let applied = crate::registry::apply(
        base.clone(),
        &parsed(json!({ "watch_files": ["**/*.css"], "watch_excludes": ["**/dist"] })),
    );
    assert_eq!(
        (applied.watch_files, applied.watch_files_from),
        (vec!["**/*.css".to_owned()], None)
    );
    assert_eq!(
        (applied.watch_excludes, applied.watch_excludes_from),
        (vec!["**/dist".to_owned()], None)
    );
    let applied = crate::registry::apply(
        base,
        &parsed(json!({ "watch_files": ["**/*.css"], "watch_files_from": "/b" })),
    );
    assert_eq!(applied.watch_files_from.as_deref(), Some("/b"));
    assert_eq!(applied.watch_excludes_from.as_deref(), Some("/a/excludes"));
}

/// The next status of `server` that isn't about readiness.
fn next_crash_status(events: &crate::tests::Events, server: ServerId) -> ServerStatus {
    loop {
        match events.status(server) {
            ServerStatus::Starting | ServerStatus::Ready | ServerStatus::Indexing { .. } => {}
            status => return status,
        }
    }
}

#[test]
fn repoll_due_follows_each_session() {
    let period = Duration::from_secs(3);
    let hour = Duration::from_secs(3600);
    let s = Clocked::new(
        ServerSpec {
            repoll: Some(period),
            ..ServerSpec::new("fake", "fake-server")
        },
        Script::default(),
    );
    let first = s.clock.now();
    s.settle();
    assert_eq!(
        s.manager.repoll_due(s.server, first + period),
        Some(first + period)
    );

    // A crash ends the session: nothing is due through its backoff.
    s.fake.crash();
    assert_eq!(
        next_crash_status(&s.events, s.server),
        ServerStatus::Restarting(1)
    );
    assert_eq!(s.manager.repoll_due(s.server, s.clock.now() + hour), None);
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_millis(999));
    assert_eq!(s.manager.repoll_due(s.server, s.clock.now() + hour), None);

    // The replacement counts from its own initialization.
    s.clock.advance(Duration::from_millis(1));
    s.fake.wait_initialized_at(2);
    let second = s.clock.now();
    assert!(second > first + period / 6, "the clock moved on");
    super::super::wait_until("the new session's poll clock", || {
        s.manager.repoll_due(s.server, second + hour).is_some()
    });
    let at = |after: Duration| s.manager.repoll_due(s.server, second + after);
    assert_eq!(at(period - Duration::from_millis(1)), None);
    assert_eq!(at(period), Some(second + period));
    assert_eq!(at(period * 2), Some(second + period * 2));

    // Crashed out of its budget (at the 6th crash in a row), it stays
    // stopped with its handle kept, and has none due.
    for n in 2..=5 {
        s.fake.crash();
        assert_eq!(
            next_crash_status(&s.events, s.server),
            ServerStatus::Restarting(n)
        );
        assert_eq!(s.manager.repoll_due(s.server, s.clock.now() + hour), None);
        let backoff = Duration::from_secs(1 << (n - 1));
        s.clock.wait_for_sleep(backoff);
        s.clock.advance(backoff);
        s.fake.wait_initialized_at(usize::try_from(n).unwrap() + 1);
    }
    s.fake.crash();
    assert!(matches!(
        next_crash_status(&s.events, s.server),
        ServerStatus::Stopped(Some(_))
    ));
    assert_eq!(s.manager.repoll_due(s.server, s.clock.now() + hour), None);
}
