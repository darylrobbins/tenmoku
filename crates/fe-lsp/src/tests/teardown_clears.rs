//! A crashed or recycled session's identities are retired, and its
//! diagnostics reported cleared, before the actor waits for the old
//! process, not after.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use fe_buffer::Buffer;

use super::{Clocked, ROOT};
use crate::fake::{Reply, Script, Scripted};
use crate::{LspEvent, ORPHANS_MAX, ServerSpec, ServerStatus};

/// A server with a synced document `a.txt` open, and the buffer's identity.
fn with_document(script: Script) -> (Clocked, Buffer, (crate::SessionId, crate::IncarnationId)) {
    let mut s = Clocked::new(ServerSpec::new("fake", "fake-server"), script);
    let mut buffer = Buffer::from_file_contents("one\n");
    let path = PathBuf::from(ROOT).join("a.txt");
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
    let uri = lsp_types::Url::from_file_path(&path).unwrap();
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let old = s.manager.active_identity(buffer.id(), s.server).unwrap();
    (s, buffer, old)
}

/// Waits for the next cleared report, and returns its identity.
fn cleared(s: &Clocked) -> (fe_buffer::BufferId, crate::SessionId, crate::IncarnationId) {
    s.events.until("diagnostics cleared", |event| match event {
        LspEvent::DiagnosticsCleared {
            buffer,
            session,
            incarnation,
            ..
        } => Some((buffer, session, incarnation)),
        _ => None,
    })
}

#[test]
fn a_recycle_clears_before_the_shutdown_wait() {
    let (s, buffer, old) = with_document(Script {
        answer_shutdown: false,
        replies: HashMap::from([(
            "textDocument/definition".to_owned(),
            Scripted::now(Reply::Never),
        )]),
        ..Script::default()
    });
    drop(super::cancel::sent_definitions(&s, 1, ORPHANS_MAX + 1));

    // The clock hasn't moved: the shutdown is still waiting out its 2 s.
    assert_eq!(cleared(&s), (buffer.id(), old.0, old.1));
    assert_ne!(s.manager.active_identity(buffer.id(), s.server), Some(old));
    assert_eq!(s.fake.kills(), 0);

    // The waits end (the fake may exit at once on `exit`, or be killed
    // after its 1 s); the session restarts, with nothing cleared twice.
    s.clock.wait_for_sleep(Duration::from_secs(2));
    s.clock.advance(Duration::from_secs(2));
    loop {
        match s.events.0.recv_timeout(Duration::from_millis(50)) {
            Ok(LspEvent::Status {
                status: ServerStatus::Restarting(1),
                ..
            }) => break,
            Ok(event) => assert!(
                !matches!(event, LspEvent::DiagnosticsCleared { .. }),
                "cleared twice: {event:?}"
            ),
            Err(_) => s.clock.advance(Duration::from_secs(1)),
        }
    }
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    s.fake.wait_initialized_at(2);
    no_second_clear(&s);
}

#[test]
fn a_crash_clears_before_the_kill_ends() {
    let (s, buffer, old) = with_document(Script::default());
    s.fake.hold_kills(true);
    s.fake.close_output();

    assert_eq!(cleared(&s), (buffer.id(), old.0, old.1));
    assert_ne!(s.manager.active_identity(buffer.id(), s.server), Some(old));
    super::wait_until("the kill", || s.fake.kills() == 1);

    s.fake.hold_kills(false);
    s.expect(ServerStatus::Restarting(1));
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    s.fake.wait_initialized_at(2);
    no_second_clear(&s);
}

/// Nothing more is reported cleared in a moment of real time.
fn no_second_clear(s: &Clocked) {
    while let Ok(event) = s.events.0.recv_timeout(Duration::from_millis(200)) {
        assert!(
            !matches!(event, LspEvent::DiagnosticsCleared { .. }),
            "cleared twice: {event:?}"
        );
    }
}
