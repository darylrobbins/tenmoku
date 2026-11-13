//! Isolation (§4 Restarts and crashes, §7 Isolation): one server of a
//! document crashing leaves the others syncing and answering, and comes
//! back within its own budget at the current text.

use std::time::Duration;

use fe_buffer::Buffer;
use lsp_types::notification::PublishDiagnostics;
use lsp_types::{Position, PublishDiagnosticsParams, Range, Url};
use serde_json::json;

use super::{CSS, Fixture, answer, echo_on};
use crate::fake::Echo;
use crate::{LspEvent, ServerId, ServerStatus};

/// `name` publishes one diagnostic for `uri` at LSP version `version`, and
/// the sink gets it for `buffer` from `server`.
fn diagnoses(f: &Fixture, name: &str, server: ServerId, uri: &Url, version: i32) {
    let message = format!("from {name}");
    f.fake(name)
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![lsp_types::Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                message: message.clone(),
                ..Default::default()
            }],
            version: Some(version),
        });
    let items = f
        .events
        .until(&format!("{name}'s diagnostics"), |event| match event {
            LspEvent::Diagnostics {
                server: from,
                items,
                ..
            } if from == server => Some(items),
            _ => None,
        });
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].message, message);
}

/// Crashes `name`, whose instance is `server`, and returns the servers
/// whose incarnations were reported cleared until it said it restarts.
pub(super) fn crash(f: &Fixture, name: &str, server: ServerId) -> Vec<ServerId> {
    f.fake(name).crash();
    let mut cleared = Vec::new();
    f.events
        .until(&format!("{name} to restart"), |event| match event {
            LspEvent::DiagnosticsCleared { server: from, .. } => {
                cleared.push(from);
                None
            }
            LspEvent::Status {
                server: from,
                status: ServerStatus::Restarting(1),
                ..
            } if from == server => Some(()),
            _ => None,
        });
    cleared
}

/// Waits out `name`'s 1 s backoff: its second launch opens the document
/// at `buffer`'s current text.
pub(super) fn comes_back(f: &Fixture, name: &str, server: ServerId, uri: &Url, buffer: &Buffer) {
    let backoff = Duration::from_secs(1);
    f.clock.wait_for_sleep(backoff);
    f.clock.advance(backoff);
    assert_eq!(f.events.status(server), ServerStatus::Starting);
    let text = buffer.text().to_string();
    f.fake(name)
        .wait_for_document(uri, "the reopen at the current text", |r| {
            r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
        });
}

/// One document on `css` and `vars`, synced to both.
fn on_both() -> (Fixture, Buffer, Url, ServerId, ServerId) {
    let mut f = Fixture::css_and_vars();
    let mut buffer = Buffer::new("a {}\n");
    let uri = f.open(&mut buffer, "a.css", CSS);
    f.wait_synced("css", &uri, &buffer);
    f.wait_synced("vars", &uri, &buffer);
    let (css, vars) = (
        f.placed(&buffer, "css").unwrap(),
        f.placed(&buffer, "vars").unwrap(),
    );
    (f, buffer, uri, css, vars)
}

#[test]
fn add_on_crash_leaves_primary_alone() {
    let (f, mut buffer, uri, css, vars) = on_both();
    let identity = f.manager.active_identity(buffer.id(), css).unwrap();

    // Only the add-on's incarnation ends.
    assert_eq!(crash(&f, "vars", vars), [vars]);
    assert_eq!(f.manager.active_identity(buffer.id(), css), Some(identity));
    assert_eq!(f.manager.active_identity(buffer.id(), vars), None);
    assert_eq!(f.manager.lead_of(buffer.id()), Some(css));

    // The primary answers, takes edits and diagnoses as before.
    assert_eq!(echo_on(&f.manager, &buffer), Ok(json!({})));
    f.edit(&mut buffer, "x");
    let replica = f.wait_synced("css", &uri, &buffer);
    diagnoses(&f, "css", css, &uri, replica.version);
    let served = f.manager.served_identities(buffer.id());
    assert_eq!(served, [(css, identity.0, identity.1)]);

    // The add-on comes back at the current text; the primary never left.
    comes_back(&f, "vars", vars, &uri, &buffer);
    assert_eq!(f.manager.active_identity(buffer.id(), css), Some(identity));
    assert_eq!(f.fake("css").launches(), 1);
    assert_eq!(f.manager.served_identities(buffer.id()).len(), 2);
    assert_eq!(f.manager.servers_of(buffer.id()), [css, vars]);
}

#[test]
fn primary_crash_leaves_add_on_alone() {
    let (f, mut buffer, uri, css, vars) = on_both();
    let identity = f.manager.active_identity(buffer.id(), vars).unwrap();

    assert_eq!(crash(&f, "css", css), [css]);
    assert_eq!(f.manager.active_identity(buffer.id(), vars), Some(identity));
    assert_eq!(f.manager.active_identity(buffer.id(), css), None);

    // The add-on answers, takes edits and diagnoses as before.
    assert_eq!(
        answer(f.manager.request::<Echo>(vars, json!({ "to": "vars" })))
            .map(|r| r.result["params"].clone()),
        Ok(json!({ "to": "vars" }))
    );
    f.edit(&mut buffer, "x");
    let replica = f.wait_synced("vars", &uri, &buffer);
    diagnoses(&f, "vars", vars, &uri, replica.version);
    let served = f.manager.served_identities(buffer.id());
    assert_eq!(served, [(vars, identity.0, identity.1)]);

    // The primary comes back at the current text; the add-on never left.
    comes_back(&f, "css", css, &uri, &buffer);
    assert_eq!(f.manager.active_identity(buffer.id(), vars), Some(identity));
    assert_eq!(f.fake("vars").launches(), 1);
    assert_eq!(f.manager.lead_of(buffer.id()), Some(css));
    assert_eq!(echo_on(&f.manager, &buffer), Ok(json!({})));
}
