//! Requests and notifications by any method name (custom.rs), through
//! async-lsp's untyped send (vendor/async-lsp): methods no table in fe-lsp
//! lists reach the server, keep their order and mirrored ids, and are
//! cancelled by the id they went with.

use std::path::Path;
use std::time::Duration;

use futures::FutureExt;
use lsp_types::NumberOrString;
use lsp_types::request::Request;
use serde_json::{Value, json};

use super::cancel::requests_of;
use super::{ROOT, Started, answer, wait_until};
use crate::fake::{Echo, Record, Reply, Script, Scripted, Writes};
use crate::requests::within;
use crate::server::QUEUED_MAX;
use crate::{LspError, ManualClock};

/// A request method no table in fe-lsp has ever listed.
const UNLISTED: &str = "acme/unlistedMethod";
/// A notification method no table in fe-lsp has ever listed.
const UNLISTED_NOTICE: &str = "acme/unlistedNotice";

/// The one id the fake got for `method`.
fn only_id(s: &Started, method: &str) -> i64 {
    match requests_of(&s.fake.records(), method).as_slice() {
        [NumberOrString::Number(id)] => i64::from(*id),
        ids => panic!("one numbered {method} request, got {ids:?}"),
    }
}

/// Where in the fake's records the first that `pick` picks is.
fn index_of(records: &[Record], pick: impl Fn(&Record) -> bool) -> usize {
    records.iter().position(pick).expect("recorded")
}

#[test]
fn a_method_in_no_table_gets_the_fake_servers_reply() {
    let s = Started::new(Script::default());
    s.fake.set_reply(
        UNLISTED,
        Scripted::now(Reply::Result(json!({ "answer": 42 }))),
    );
    // A typed request first, then the raw one, then a typed one again: the
    // raw request's id comes from async-lsp's counter, between theirs.
    s.barrier();
    let reply = answer(s.manager.custom_request_at(
        "fake",
        Path::new(ROOT),
        UNLISTED,
        json!({ "question": "everything" }),
    ));
    assert_eq!(reply, Ok(json!({ "answer": 42 })));
    s.barrier();

    let records = s.fake.records();
    let echoes = requests_of(&records, Echo::METHOD);
    assert_eq!(
        echoes,
        [only_id(&s, UNLISTED) - 1, only_id(&s, UNLISTED) + 1]
            .map(|id| NumberOrString::Number(i32::try_from(id).unwrap())),
        "the raw request's id is the next one, between the typed requests'",
    );
    let params = records.iter().find_map(|record| match record {
        Record::Request { method, params, .. } if method == UNLISTED => Some(params.clone()),
        _ => None,
    });
    assert_eq!(params, Some(json!({ "question": "everything" })));

    // A null result is a result, not a missing one.
    s.fake
        .set_reply(UNLISTED, Scripted::now(Reply::Result(Value::Null)));
    let reply = answer(
        s.manager
            .custom_request_at("fake", Path::new(ROOT), UNLISTED, json!({})),
    );
    assert_eq!(reply, Ok(Value::Null));
}

#[test]
fn a_notification_in_no_table_reaches_the_fake_server() {
    let s = Started::new(Script::default());
    // async-lsp's queue full and the actor holding more echoes back: the
    // notification waits in the window behind every one of them, as the
    // editor's own notifications do, rather than overtaking those held.
    s.fake.set_writes(Writes::Held);
    let echoes: Vec<_> = (0..2 * QUEUED_MAX)
        .map(|n| s.manager.request::<Echo>(s.server, json!({ "n": n })))
        .collect();
    s.manager
        .custom_notify_at(
            "fake",
            Path::new(ROOT),
            UNLISTED_NOTICE,
            json!({ "flag": true }),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        !s.fake
            .records()
            .iter()
            .any(|record| record.is_notification(UNLISTED_NOTICE)),
        "nothing reaches the fake while its input is held",
    );
    s.fake.set_writes(Writes::Open);
    for echo in echoes {
        answer(echo).unwrap();
    }
    s.barrier();

    let records = s.fake.records();
    let notice = index_of(&records, |record| record.is_notification(UNLISTED_NOTICE));
    let last_echo = (records.iter())
        .rposition(|record| {
            matches!(record, Record::Request { method, params, .. }
            if method == Echo::METHOD && params.get("n").is_some())
        })
        .expect("echoes");
    assert!(
        last_echo < notice,
        "the notification comes after the requests queued before it"
    );
    match &records[notice] {
        Record::Notification { params, .. } => assert_eq!(params, &json!({ "flag": true })),
        other => panic!("not a notification: {other:?}"),
    }
    // It consumed no request id: the echoes' and the barrier's ids run on
    // without a gap.
    let ids: Vec<_> = requests_of(&records, Echo::METHOD);
    let numbers: Vec<i32> = (ids.iter())
        .map(|id| match id {
            NumberOrString::Number(id) => *id,
            NumberOrString::String(id) => panic!("a string id {id}"),
        })
        .collect();
    assert!(
        numbers.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "{numbers:?}"
    );
}

#[test]
fn a_timed_out_raw_request_is_cancelled_by_its_wire_id() {
    let s = Started::new(Script::default());
    s.fake.set_reply(UNLISTED, Scripted::now(Reply::Never));
    s.barrier();
    // The caller's own deadline, as fe-workbench's plugin host keeps one.
    let deadline = ManualClock::new();
    let mut timed = within(
        s.manager
            .custom_request_at("fake", Path::new(ROOT), UNLISTED, json!({})),
        Some(Duration::from_secs(5)),
        &deadline,
        Err(LspError::Timeout),
    )
    .boxed();
    s.fake.wait_for("the raw request", |records| {
        requests_of(records, UNLISTED).len() == 1
    });
    let id = only_id(&s, UNLISTED);
    assert!(timed.as_mut().now_or_never().is_none());
    deadline.advance(Duration::from_secs(5));
    assert_eq!(answer(timed), Err(LspError::Timeout));

    wait_until("the raw request's cancel", || {
        s.fake.records().iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == json!(id))
        })
    });
    // The ids stay mirrored: a typed request after it is answered.
    s.barrier();
    let echoes = requests_of(&s.fake.records(), Echo::METHOD);
    assert_eq!(
        echoes.last(),
        Some(&NumberOrString::Number(i32::try_from(id + 1).unwrap()))
    );
}
