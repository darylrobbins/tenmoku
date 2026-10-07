//! What the client answers and ignores, and request-id mirroring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use lsp_types::notification::Notification;
use lsp_types::request::{
    ApplyWorkspaceEdit, CodeLensRefresh, InlayHintRefreshRequest, Request, SemanticTokensRefresh,
    ShowDocument, ShowMessageRequest, WorkDoneProgressCreate, WorkspaceDiagnosticRefresh,
    WorkspaceFoldersRequest,
};
use lsp_types::{
    ApplyWorkspaceEditParams, MessageType, NumberOrString, ShowDocumentParams,
    ShowMessageRequestParams, Url, WorkDoneProgressCreateParams, WorkspaceEdit,
};
use serde_json::{Value, json};

use super::{capture_logs, logs_from, manager, wait_until};
use crate::fake::{Echo, FakeError, FakeServer, Record, Reply, Script, Scripted};
use crate::{LspError, ManualClock, ServerSpec};

macro_rules! notification {
    ($name:ident, $method:literal) => {
        enum $name {}
        impl Notification for $name {
            type Params = Value;
            const METHOD: &'static str = $method;
        }
    };
}

notification!(TelemetryEvent, "telemetry/event");
notification!(CustomUnknown, "custom/unknown");
notification!(TypescriptVersion, "$/typescriptVersion");

macro_rules! request {
    ($name:ident, $method:literal) => {
        enum $name {}
        impl Request for $name {
            type Params = Value;
            type Result = Value;
            const METHOD: &'static str = $method;
        }
    };
}

request!(Answered, "custom/answered");
request!(Failed, "custom/failed");
request!(Cancelled, "custom/cancelled");
request!(Modified, "custom/modified");
request!(Delayed, "custom/delayed");

enum CustomRequest {}

impl Request for CustomRequest {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "custom/request";
}

#[test]
fn unknown_notifications_do_not_stop_the_loop() {
    capture_logs("fe-lsp notifications");
    let root = tempfile::tempdir().unwrap();
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = manager(fake.launcher(), None);
    let id = manager
        .start(ServerSpec::new("notifications", "fake-server"), root.path())
        .unwrap();
    fake.wait_initialized();

    fake.notify::<TelemetryEvent>(json!({ "event": "started" }));
    fake.notify::<CustomUnknown>(json!(null));
    fake.notify::<TypescriptVersion>(json!({ "version": "6.0.3" }));
    let reply = async_io::block_on(manager.request::<Echo>(id, json!({ "after": true })));
    assert_eq!(reply.unwrap()["params"], json!({ "after": true }));

    for method in ["telemetry/event", "custom/unknown", "$/typescriptVersion"] {
        wait_until(&format!("{method} to be logged"), || {
            logs_from("fe-lsp notifications")
                .iter()
                .any(|line| line.contains(method))
        });
    }
}

#[test]
fn server_requests_get_the_m4_answers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = manager(fake.launcher(), None);
    manager
        .start(ServerSpec::new("server-requests", "fake-server"), &root)
        .unwrap();
    fake.wait_initialized();

    let token = WorkDoneProgressCreateParams {
        token: NumberOrString::String("rustAnalyzer/Indexing".into()),
    };
    assert_eq!(
        fake.request::<WorkDoneProgressCreate>(token),
        Ok(Value::Null)
    );
    let uri = Url::from_file_path(&root).unwrap();
    assert_eq!(
        fake.request::<WorkspaceFoldersRequest>(()),
        Ok(json!([{ "uri": uri, "name": "workspace" }]))
    );
    let edit = ApplyWorkspaceEditParams {
        label: None,
        edit: WorkspaceEdit::default(),
    };
    assert_eq!(
        fake.request::<ApplyWorkspaceEdit>(edit),
        Ok(json!({ "applied": false, "failureReason": "not supported" }))
    );
    let message = ShowMessageRequestParams {
        typ: MessageType::INFO,
        message: "Reload the workspace?".into(),
        actions: None,
    };
    assert_eq!(fake.request::<ShowMessageRequest>(message), Ok(Value::Null));
    let document = ShowDocumentParams {
        uri: uri.clone(),
        external: None,
        take_focus: None,
        selection: None,
    };
    assert_eq!(
        fake.request::<ShowDocument>(document),
        Ok(json!({ "success": false }))
    );
    assert_eq!(
        fake.request::<WorkspaceDiagnosticRefresh>(()),
        Ok(Value::Null)
    );
    assert_eq!(fake.request::<SemanticTokensRefresh>(()), Ok(Value::Null));
    assert_eq!(fake.request::<InlayHintRefreshRequest>(()), Ok(Value::Null));
    assert_eq!(fake.request::<CodeLensRefresh>(()), Ok(Value::Null));
    let unknown = fake.request::<CustomRequest>(json!({}));
    assert!(
        matches!(
            unknown,
            Err(FakeError {
                code: Some(-32601),
                ..
            })
        ),
        "{unknown:?}"
    );
}

#[test]
fn mirrored_request_ids_match() {
    let root = tempfile::tempdir().unwrap();
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = manager(fake.launcher(), None);
    let id = manager
        .start(ServerSpec::new("ids", "fake-server"), root.path())
        .unwrap();
    // Issued before `initialized`, the first ones wait for it.
    let replies: Vec<_> = (0..20u64)
        .map(|n| {
            // The odd ones answer late, so replies arrive out of order.
            let delay = if n % 2 == 1 { 40 - 2 * n } else { 0 };
            manager.request_with_id::<Echo>(id, json!({ "n": n, "delay_ms": delay }))
        })
        .collect();
    let replies = async_io::block_on(futures::future::join_all(replies));

    let records = fake.records();
    for (n, reply) in replies.into_iter().enumerate() {
        let (mirrored, result) = reply.unwrap();
        let echoed = result.unwrap();
        assert_eq!(echoed["params"]["n"], json!(n));
        let received = records
            .iter()
            .find_map(|r| match r {
                Record::Request { id, method, params }
                    if method == Echo::METHOD && params["n"] == json!(n) =>
                {
                    Some(id.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(received, NumberOrString::Number(mirrored), "request {n}");
        assert_eq!(echoed["id"], json!(mirrored));
    }
    let reply_order: Vec<_> = records
        .iter()
        .filter_map(|r| match r {
            Record::Replied {
                id: NumberOrString::Number(id),
                method,
            } if method == Echo::METHOD => Some(*id),
            _ => None,
        })
        .collect();
    let mut sorted = reply_order.clone();
    sorted.sort_unstable();
    assert_ne!(reply_order, sorted, "replies came in order");

    // With requests still unanswered when the server stops, `shutdown`
    // gets the id after theirs, and their callers hear `ServerDown`.
    let pending: Vec<_> = (0..20)
        .map(|n| manager.request::<Echo>(id, json!({ "n": n, "never": true })))
        .collect();
    fake.wait_for("the unanswered requests", |records| {
        let requests = records
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == Echo::METHOD));
        requests.count() == 40
    });
    manager.stop(id);
    let records = fake.wait_for("shutdown", |records| {
        records.iter().any(|r| r.method() == "shutdown")
    });
    let shutdown = records
        .iter()
        .find_map(|r| match r {
            Record::Request { id, method, .. } if method == "shutdown" => Some(id.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(shutdown, NumberOrString::Number(41));
    for reply in pending {
        assert_eq!(async_io::block_on(reply), Err(LspError::ServerDown));
    }
}

#[test]
fn scripted_replies_reach_the_caller() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new());
    let error = |code: i32, message: &str| {
        Scripted::now(Reply::Error {
            code,
            message: message.into(),
        })
    };
    let script = Script {
        replies: HashMap::from([
            (
                Answered::METHOD.to_owned(),
                Scripted::now(Reply::Result(json!({ "items": [1, 2, 3] }))),
            ),
            (Failed::METHOD.to_owned(), error(-32603, "it broke")),
            (Cancelled::METHOD.to_owned(), error(-32800, "cancelled")),
            (
                Modified::METHOD.to_owned(),
                error(-32801, "content modified"),
            ),
            (
                Delayed::METHOD.to_owned(),
                Scripted {
                    reply: Reply::Result(json!("late")),
                    delay: Duration::from_secs(5),
                },
            ),
        ]),
        ..Script::default()
    };
    let fake = FakeServer::with_clock(script, clock.clone());
    let (mut manager, _events) = manager(fake.launcher(), Some(clock.clone()));
    let id = manager
        .start(ServerSpec::new("scripted", "fake-server"), root.path())
        .unwrap();

    assert_eq!(
        async_io::block_on(manager.request::<Answered>(id, json!({}))),
        Ok(json!({ "items": [1, 2, 3] }))
    );
    assert_eq!(
        async_io::block_on(manager.request::<Failed>(id, json!({}))),
        Err(LspError::Server {
            code: -32603,
            message: "it broke".into()
        })
    );
    assert_eq!(
        async_io::block_on(manager.request::<Cancelled>(id, json!({}))),
        Err(LspError::Cancelled)
    );
    assert_eq!(
        async_io::block_on(manager.request::<Modified>(id, json!({}))),
        Err(LspError::Retry)
    );
    // The fake's delays run on its clock.
    let late = manager.request::<Delayed>(id, json!({}));
    clock.wait_for_sleep(Duration::from_secs(5));
    clock.advance(Duration::from_secs(5));
    assert_eq!(async_io::block_on(late), Ok(json!("late")));
}
