//! Status events, the log ring and `window/showMessage` (§3.5).

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use lsp_types::notification::{LogMessage, ShowMessage};
use lsp_types::{LogMessageParams, MessageType, ShowMessageParams};

use super::readiness::{begin, end};
use super::{Clocked, ROOT, manager, wait_until};
use crate::fake::Script;
use crate::router::{ServerStatusNotification, ServerStatusParams};
use crate::{
    LspEvent, ManualClock, ReadinessMode, ServerIo, ServerLauncher, ServerSpec, ServerStatus,
};

/// A launcher with nothing to run.
struct Missing;

impl ServerLauncher for Missing {
    fn launch(&self, _: &ServerSpec, _: &Path) -> io::Result<ServerIo> {
        Err(io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }
}

#[test]
fn status_and_log_events() {
    // Starting, Indexing while progress is open, then Ready (by status
    // here, so with the progress still open).
    let s = Clocked::new(
        ServerSpec {
            readiness: ReadinessMode::ServerStatus,
            ..ServerSpec::new("fake", "fake-server")
        },
        Script::default(),
    );
    begin(&s.fake, "load", "Loading");
    s.expect(ServerStatus::Indexing {
        title: "Loading".into(),
    });
    s.fake
        .notify::<ServerStatusNotification>(ServerStatusParams {
            health: "ok".into(),
            quiescent: true,
            message: None,
        });
    s.expect(ServerStatus::Ready);
    end(&s.fake, "load");

    // Restarting(2) after a second crash.
    s.fake.crash();
    s.expect(ServerStatus::Restarting(1));
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    s.fake.wait_initialized_at(2);
    s.fake.crash();
    s.expect(ServerStatus::Restarting(2));

    // A server that isn't there.
    let (mut manager, events) = manager(Arc::new(Missing), Some(Arc::new(ManualClock::new())));
    let id = manager
        .start(
            ServerSpec::new("rust-analyzer", "rust-analyzer"),
            Path::new(ROOT),
        )
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    assert_eq!(
        events.status(id),
        ServerStatus::Stopped(Some("rust-analyzer not found".into()))
    );

    // stderr and window/logMessage, the last 64 KiB of them.
    let s = Clocked::new(ServerSpec::new("logged", "fake-server"), Script::default());
    let log = || s.manager.log("logged", Path::new(ROOT));
    s.fake.stderr(b"from stderr\n");
    s.fake.notify::<LogMessage>(LogMessageParams {
        typ: MessageType::INFO,
        message: "logged".into(),
    });
    wait_until("both in the log", || {
        let log = log();
        log.contains("from stderr\n") && log.contains("logged")
    });
    for n in 0..100 {
        let line = format!("{n:03} {}\n", "x".repeat(1000));
        s.fake.stderr(line.as_bytes());
    }
    wait_until("the last line", || log().contains("099 x"));
    let kept = log();
    assert!(kept.len() <= 64 * 1024, "{}", kept.len());
    assert!(kept.len() > 63 * 1024, "{}", kept.len());
    assert!(kept.ends_with(&format!("099 {}\n", "x".repeat(1000))));
    assert!(!kept.contains("from stderr"));

    // window/showMessage.
    s.fake.notify::<ShowMessage>(ShowMessageParams {
        typ: MessageType::WARNING,
        message: "careful".into(),
    });
    let shown = s.events.until("a ShowMessage", |event| match event {
        LspEvent::ShowMessage {
            server,
            name,
            typ,
            message,
        } => Some((server, name, typ, message)),
        _ => None,
    });
    assert_eq!(
        shown,
        (
            s.server,
            "logged".to_owned(),
            MessageType::WARNING,
            "careful".to_owned()
        )
    );
}

#[test]
fn a_crashing_server_keeps_its_last_words() {
    let s = Clocked::new(ServerSpec::new("dying", "fake-server"), Script::default());
    // Far more than a pipe holds: a proxy may buffer any amount.
    let line = format!("{}\n", "y".repeat(1000));
    for _ in 0..2000 {
        s.fake.stderr(line.as_bytes());
    }
    s.fake.stderr(b"thread 'main' panicked: the last words\n");
    s.fake.crash();
    s.expect(ServerStatus::Restarting(1));
    let log = s.manager.log("dying", Path::new(ROOT));
    assert!(
        log.ends_with("the last words\n"),
        "{}",
        &log[log.len() - 100..]
    );
    assert!(log.len() > 63 * 1024 && log.len() <= 64 * 1024);
}
