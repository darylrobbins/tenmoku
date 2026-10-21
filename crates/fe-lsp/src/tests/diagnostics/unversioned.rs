//! A publication without a version (§4.1 item 4): decoded against the text
//! last sent when the server's diagnostics aren't pulled, dropped otherwise.

use std::path::Path;
use std::sync::Arc;

use fe_buffer::Buffer;
use lsp_types::{DiagnosticOptions, DiagnosticServerCapabilities, PositionEncodingKind};

use super::{marker, next, over, publish};
use crate::fake::{FakeServer, InitializeReply, Script, initialize_result};
use crate::tests::{ROOT, Started, manager};
use crate::{Clock, ManualClock, ServerSpec, ServerStatus, UnversionedDiagnostics};

/// A server on the fake, advertising `diagnosticProvider` if `provider`,
/// started with `unversioned` unless it's the default.
fn started(provider: bool, unversioned: Option<UnversionedDiagnostics>) -> Started {
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
    let fake = FakeServer::new(script);
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock));
    let mut spec = ServerSpec::new("fake", "fake-server");
    if let Some(unversioned) = unversioned {
        spec.unversioned_diagnostics = unversioned;
    }
    let server = manager.start(spec, Path::new(ROOT)).unwrap();
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    Started {
        fake,
        manager,
        server,
        events,
    }
}

#[test]
fn unversioned_pushes_land_on_the_text_last_sent() {
    let mut s = started(false, None);
    let mut buffer = Buffer::new("one two\n");
    let at_open = buffer.version();
    let uri = s.open_synced(&mut buffer, "a.txt");
    let v0 = buffer.text().to_string();

    publish(&s, &uri, None, vec![over(&v0, "two", false, "before")]);
    let published = next(&s.events);
    assert_eq!(published.version, at_open);
    assert_eq!(published.items.len(), 1);
    assert_eq!(published.items[0].message, "before");
    assert_eq!(&v0[published.items[0].range.clone()], "two");

    s.edit(&mut buffer, &uri, (0..0, "zero "), 1);
    s.edit(&mut buffer, &uri, (0..0, "minus "), 2);
    let v2 = buffer.text().to_string();

    publish(&s, &uri, None, vec![over(&v2, "two", false, "after")]);
    let published = next(&s.events);
    assert_eq!(published.version, buffer.version());
    assert_eq!(published.items[0].message, "after");
    assert_eq!(&v2[published.items[0].range.clone()], "two");

    // Older than the newest decoded: still out of order.
    publish(&s, &uri, Some(1), vec![over(&v2, "two", false, "stale")]);
    marker(&s, &uri, 2, &v2, "an out-of-order publication");
}

#[test]
fn a_pulling_server_still_drops_unversioned_pushes() {
    let mut s = started(true, None);
    let mut buffer = Buffer::new("one two\n");
    let uri = s.open_synced(&mut buffer, "a.txt");
    let v0 = buffer.text().to_string();
    publish(&s, &uri, None, vec![over(&v0, "two", false, "unversioned")]);
    marker(&s, &uri, 0, &v0, "an unversioned push from a pulled server");
}

#[test]
fn a_drop_spec_drops_unversioned_pushes() {
    let mut s = started(false, Some(UnversionedDiagnostics::Drop));
    let mut buffer = Buffer::new("one two\n");
    let uri = s.open_synced(&mut buffer, "a.txt");
    let v0 = buffer.text().to_string();

    // Before the first edit.
    publish(&s, &uri, None, vec![over(&v0, "two", false, "unversioned")]);
    marker(
        &s,
        &uri,
        0,
        &v0,
        "an unversioned publication before any edit",
    );

    s.edit(&mut buffer, &uri, (0..0, "zero "), 1);
    s.edit(&mut buffer, &uri, (0..0, "minus "), 2);
    let v2 = buffer.text().to_string();

    // After one.
    publish(&s, &uri, None, vec![over(&v2, "two", false, "unversioned")]);
    marker(&s, &uri, 2, &v2, "an unversioned publication after an edit");

    // Computed on the old incarnation's text, sent after the new didOpen
    // and before any didChange.
    s.manager.close(buffer.id());
    s.fake
        .wait_for_document(&uri, "didClose", |r| r.is_some_and(|r| !r.open));
    let old_text = buffer.text().to_string();
    buffer
        .edit([(0..0, "new ")], fe_buffer::Origin::User)
        .unwrap();
    let new_text = buffer.text().to_string();
    s.open_synced(&mut buffer, "a.txt");
    assert_eq!(s.fake.document(&uri).unwrap().text, new_text);
    publish(
        &s,
        &uri,
        None,
        vec![over(&old_text, "two", false, "old text")],
    );
    marker(
        &s,
        &uri,
        3,
        &new_text,
        "an unversioned publication across a reopen",
    );
}
