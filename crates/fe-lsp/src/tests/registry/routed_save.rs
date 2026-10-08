//! A save of the opened version survives a routed open and a handoff.

use std::path::Path;
use std::sync::Arc;

use fe_buffer::{Buffer, Origin};
use fe_config::settings::LspSettings;
use serde_json::json;

use super::{MissingOrFake, barrier, open, root, routed, server, uri};
use crate::fake::{FakeServer, Script};
use crate::tests::{ROOT, manager, wait_until};
use crate::{ManualClock, ServerStatus};

/// A buffer subscribed early, edited and saved, then opened through
/// `LspManager::open`: `didOpen` carries the current text, and the save of
/// that version follows. A save a later edit superseded isn't sent.
#[test]
fn a_routed_open_keeps_the_save_of_the_opened_version() {
    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());

    let mut saved = Buffer::new("one\n");
    let subscription = saved.subscribe();
    saved.edit([(0..0, "1")], Origin::User).unwrap();
    saved.did_save();
    let saved_path = format!("{ROOT}/saved.py");
    let pyright = manager
        .open(
            root(),
            saved.id(),
            saved.text().clone(),
            saved.version(),
            subscription,
            Path::new(&saved_path),
            "python",
        )
        .unwrap();
    let replica = fake.wait_for_document(&uri(&saved_path), "didSave", |r| {
        r.is_some_and(|r| !r.saves.is_empty())
    });
    let replica = replica.unwrap();
    assert_eq!(replica.text, "1one\n");
    assert_eq!(replica.saves.len(), 1);

    let mut superseded = Buffer::new("two\n");
    let subscription = superseded.subscribe();
    superseded.edit([(0..0, "1")], Origin::User).unwrap();
    superseded.did_save();
    superseded.edit([(0..0, "2")], Origin::User).unwrap();
    let superseded_path = format!("{ROOT}/superseded.py");
    manager
        .open(
            root(),
            superseded.id(),
            superseded.text().clone(),
            superseded.version(),
            subscription,
            Path::new(&superseded_path),
            "python",
        )
        .unwrap();
    let replica = fake
        .wait_for_document(&uri(&superseded_path), "didOpen", |r| r.is_some())
        .unwrap();
    barrier(&manager, pyright.unwrap());
    let replica_now = fake
        .wait_for_document(&uri(&superseded_path), "didOpen", |r| r.is_some())
        .unwrap();
    assert_eq!(replica.text, "21two\n");
    assert!(replica_now.saves.is_empty(), "{replica_now:?}");
}

/// A document handed on to a server that starts later gets its queued
/// save of the version its `didOpen` carries; one edited after its save
/// doesn't.
#[test]
fn a_handoff_keeps_the_save_of_the_caught_up_version() {
    let fake = FakeServer::new(Script::default());
    let launcher = Arc::new(MissingOrFake(fake.launcher()));
    let (mut manager, events) = manager(launcher, Some(Arc::new(ManualClock::new())));
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("pyright".into(), server(json!({ "command": "missing" })));
    manager.update_settings(&settings);

    let mut saved = Buffer::new("one\n");
    let mut superseded = Buffer::new("two\n");
    let saved_path = format!("{ROOT}/saved.py");
    let superseded_path = format!("{ROOT}/superseded.py");
    let failed = open(&mut manager, root(), &mut saved, &saved_path, "python").unwrap();
    open(
        &mut manager,
        root(),
        &mut superseded,
        &superseded_path,
        "python",
    );
    assert_eq!(events.status(failed), ServerStatus::Starting);
    assert_eq!(
        events.status(failed),
        ServerStatus::Stopped(Some("pyright not found".into()))
    );
    wait_until("the documents to wait", || {
        manager.server_of(saved.id()).is_none() && manager.server_of(superseded.id()).is_none()
    });

    // Nothing drains these before the server comes back.
    saved.edit([(0..0, "1")], Origin::User).unwrap();
    saved.did_save();
    superseded.edit([(0..0, "1")], Origin::User).unwrap();
    superseded.did_save();
    superseded.edit([(0..0, "2")], Origin::User).unwrap();

    settings.servers.get_mut("pyright").unwrap().command = Some("pyright-langserver".into());
    manager.update_settings(&settings);
    let replica = fake
        .wait_for_document(&uri(&saved_path), "didSave", |r| {
            r.is_some_and(|r| !r.saves.is_empty())
        })
        .unwrap();
    assert_eq!(replica.text, "1one\n");
    assert_eq!(replica.saves.len(), 1);
    let replica = fake
        .wait_for_document(&uri(&superseded_path), "didOpen", |r| r.is_some())
        .unwrap();
    barrier(&manager, manager.server_of(superseded.id()).unwrap());
    let replica_now = fake
        .wait_for_document(&uri(&superseded_path), "didOpen", |r| r.is_some())
        .unwrap();
    assert_eq!(replica.text, "21two\n");
    assert!(replica_now.saves.is_empty(), "{replica_now:?}");
}
