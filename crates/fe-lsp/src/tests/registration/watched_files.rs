//! `workspace/didChangeWatchedFiles`: registered globs and kinds pick the
//! file changes each server is sent.

use std::path::PathBuf;

use lsp_types::FileChangeType;
use lsp_types::request::{RegisterCapability, UnregisterCapability};
use lsp_types::{Registration, RegistrationParams, Unregistration, UnregistrationParams};
use serde_json::{Value, json};

use super::WATCHED;
use crate::fake::{FakeError, Record, Script};
use crate::tests::{ROOT, Started};

const MARKER: &str = "workspace/didCreateFiles";

fn register(s: &Started, id: &str, options: Value) -> Result<Value, FakeError> {
    s.fake.request::<RegisterCapability>(RegistrationParams {
        registrations: vec![Registration {
            id: id.into(),
            method: WATCHED.into(),
            register_options: Some(options),
        }],
    })
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(ROOT).join(name)
}

fn uri(name: &str) -> String {
    crate::fake::fake_uri(name)
}

/// The watched-files notifications the fake has been sent, once a marker
/// sent after them has arrived.
fn sent(s: &Started) -> Vec<Value> {
    let markers = |records: &[Record]| {
        records
            .iter()
            .filter(|record| record.is_notification(MARKER))
            .count()
    };
    let before = markers(&s.fake.records());
    s.manager
        .notify_sessions(MARKER, |_, _| Some(json!({ "files": [] })));
    s.fake
        .wait_for("the marker", |records| markers(records) > before);
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Notification { method, params } if method == WATCHED => Some(params),
            _ => None,
        })
        .collect()
}

#[test]
fn registered_globs_get_matching_changes_only() {
    let s = Started::new(Script::default());
    let options = json!({ "watchers": [
        { "globPattern": "**/*.rs" },
        { "globPattern": { "baseUri": crate::fake::fake_uri("cfg"), "pattern": "*.toml" } },
    ] });
    assert_eq!(register(&s, "w", options), Ok(Value::Null));
    s.manager.files_changed(vec![
        (path("src/a.rs"), FileChangeType::CREATED),
        (path("cfg/x.toml"), FileChangeType::CHANGED),
        (path("README.md"), FileChangeType::CHANGED),
        (path("cfg/sub/y.toml"), FileChangeType::DELETED),
    ]);
    assert_eq!(
        sent(&s),
        [json!({ "changes": [
            { "uri": uri("src/a.rs"), "type": 1 },
            { "uri": uri("cfg/x.toml"), "type": 2 },
        ] })]
    );

    // A path given twice is sent once, in its place, with its last type.
    s.manager.files_changed(vec![
        (path("b.rs"), FileChangeType::CREATED),
        (path("c.rs"), FileChangeType::CHANGED),
        (path("b.rs"), FileChangeType::DELETED),
    ]);
    assert_eq!(
        sent(&s)[1],
        json!({ "changes": [
            { "uri": uri("b.rs"), "type": 3 },
            { "uri": uri("c.rs"), "type": 2 },
        ] })
    );
}

#[test]
fn kind_bits_filter_events() {
    let s = Started::new(Script::default());
    let options = json!({ "watchers": [{ "globPattern": "**/*.go", "kind": 4 }] });
    assert_eq!(register(&s, "w", options), Ok(Value::Null));
    s.manager.files_changed(vec![
        (path("a.go"), FileChangeType::CREATED),
        (path("b.go"), FileChangeType::DELETED),
        (path("c.go"), FileChangeType::CHANGED),
        (path("d.go"), FileChangeType::DELETED),
    ]);
    assert_eq!(
        sent(&s),
        [json!({ "changes": [
            { "uri": uri("b.go"), "type": 3 },
            { "uri": uri("d.go"), "type": 3 },
        ] })]
    );

    // Nothing it wants: nothing is sent.
    s.manager
        .files_changed(vec![(path("a.go"), FileChangeType::CHANGED)]);
    assert_eq!(sent(&s).len(), 1);
}

#[test]
fn unregistering_stops_them() {
    let s = Started::new(Script::default());
    // With no registration, nothing is sent.
    s.manager
        .files_changed(vec![(path("a.rs"), FileChangeType::CHANGED)]);
    assert!(sent(&s).is_empty());

    let options = json!({ "watchers": [{ "globPattern": "**/*.rs" }] });
    assert_eq!(register(&s, "w", options), Ok(Value::Null));
    assert_eq!(s.manager.registrations(s.server), ["w"]);
    s.manager
        .files_changed(vec![(path("a.rs"), FileChangeType::CHANGED)]);
    assert_eq!(sent(&s).len(), 1);

    let unregistered = s
        .fake
        .request::<UnregisterCapability>(UnregistrationParams {
            unregisterations: vec![Unregistration {
                id: "w".into(),
                method: WATCHED.into(),
            }],
        });
    assert_eq!(unregistered, Ok(Value::Null));
    assert!(s.manager.registrations(s.server).is_empty());
    s.manager
        .files_changed(vec![(path("a.rs"), FileChangeType::CHANGED)]);
    assert_eq!(sent(&s).len(), 1);

    // A bad glob refuses the registration, which records nothing, even
    // its good watchers.
    let options = json!({ "watchers": [
        { "globPattern": "**/*.rs" },
        { "globPattern": "[" },
    ] });
    let refused = register(&s, "bad", options).unwrap_err();
    assert_eq!(refused.code, Some(-32803), "{refused:?}");
    assert!(refused.message.contains(WATCHED), "{refused:?}");
    // So do options that don't parse.
    let refused = register(&s, "bad", json!({ "watchers": "**/*.rs" })).unwrap_err();
    assert_eq!(refused.code, Some(-32803), "{refused:?}");
    assert!(s.manager.registrations(s.server).is_empty());
    s.manager
        .files_changed(vec![(path("a.rs"), FileChangeType::CHANGED)]);
    assert_eq!(sent(&s).len(), 1);
}

#[test]
fn watched_files_are_advertised() {
    let s = Started::new(Script::default());
    let init = (s.fake.records().into_iter())
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        init["capabilities"]["workspace"]["didChangeWatchedFiles"],
        json!({ "dynamicRegistration": true, "relativePatternSupport": true })
    );
}
