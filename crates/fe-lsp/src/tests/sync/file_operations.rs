//! File operations: `didCreateFiles`, `didRenameFiles` and
//! `didDeleteFiles` follow each server's filters, `willRenameFiles` gives
//! decoded edits, and the capabilities are advertised.

use std::path::PathBuf;
use std::time::Duration;

use futures::FutureExt;
use lsp_types::Url;
use serde_json::{Value, json};

use super::{ROOT, Session};
use crate::fake::{InitializeReply, Record, Reply, Script, Scripted};
use crate::tests::answer;

/// A fake that registers `operations` (the `workspace.fileOperations`
/// server capability).
fn script(operations: Value) -> Script {
    let mut result = crate::fake::initialize_result(None);
    result.capabilities.workspace =
        Some(serde_json::from_value(json!({ "fileOperations": operations })).unwrap());
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(ROOT).join(name)
}

fn uri(name: &str) -> Url {
    Url::from_file_path(path(name)).unwrap()
}

fn filter(glob: &str, matches: &str) -> Value {
    json!({"filters": [{"scheme": "file", "pattern": {"glob": glob, "matches": matches}}]})
}

/// The params of the notifications the fake got for `method`.
fn notified(session: &Session, method: &str) -> Vec<Value> {
    session.barrier();
    session
        .fake
        .records()
        .into_iter()
        .filter_map(|record| match record {
            Record::Notification { method: m, params } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

#[test]
fn did_notifications_follow_server_filters() {
    let session = Session::new(script(json!({
        "didRename": filter("**/*.rs", "file"),
        "didCreate": filter("**", "folder"),
    })));

    session.manager.did_rename_files(vec![
        (path("a.rs"), path("b.rs"), false),
        (path("x.txt"), path("y.txt"), false),
    ]);
    assert_eq!(
        notified(&session, "workspace/didRenameFiles"),
        [json!({"files": [{"oldUri": uri("a.rs"), "newUri": uri("b.rs")}]})]
    );

    session
        .manager
        .did_create_files(vec![(path("m"), true), (path("n.rs"), false)]);
    assert_eq!(
        notified(&session, "workspace/didCreateFiles"),
        [json!({"files": [{"uri": uri("m")}]})]
    );

    session
        .manager
        .did_delete_files(vec![(path("a.rs"), false), (path("m"), true)]);
    assert_eq!(
        notified(&session, "workspace/didDeleteFiles"),
        Vec::<Value>::new()
    );
}

#[test]
fn will_rename_returns_decoded_edits_or_times_out() {
    let edit = |text: &str| {
        json!([{"range": {"start": {"line": 0, "character": 0},
        "end": {"line": 0, "character": 0}}, "newText": text}])
    };
    let workspace_edit = json!({"changes": {
        uri("open.rs"): edit("// "),
        uri("closed.rs"): edit("// "),
    }});
    let mut script = script(json!({"willRename": filter("**/*.rs", "file")}));
    script.replies.insert(
        "workspace/willRenameFiles".into(),
        Scripted::now(Reply::Result(workspace_edit.clone())),
    );
    let mut session = Session::new(script);
    let _open = session.open("open.rs", "fn a() {}\n", "rust");

    let moves = vec![(path("a.rs"), path("b.rs"), false)];
    let edits = answer(session.manager.will_rename_files(moves.clone()));
    let applied = answer(session.manager.decode_workspace_edit(
        session.server,
        serde_json::from_value(workspace_edit).unwrap(),
    ))
    .unwrap();
    assert_eq!(edits.len(), 2);
    assert_eq!(edits, applied);
    let request = session
        .fake
        .records()
        .into_iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "workspace/willRenameFiles" => {
                Some(params)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        request,
        json!({"files": [{"oldUri": uri("a.rs"), "newUri": uri("b.rs")}]})
    );

    // A rename no filter matches asks nothing.
    let other = vec![(path("x.txt"), path("y.txt"), false)];
    assert!(answer(session.manager.will_rename_files(other)).is_empty());

    // An answer past `lsp.timeouts.references` gives nothing, and is
    // cancelled.
    session.fake.set_reply(
        "workspace/willRenameFiles",
        Scripted {
            reply: Reply::Result(json!({"changes": {}})),
            delay: Duration::from_secs(600),
        },
    );
    let mut late = session.manager.will_rename_files(moves).boxed();
    session
        .fake
        .wait_for("a second willRenameFiles", |records| {
            records
                .iter()
                .filter(|record| {
                    matches!(record, Record::Request { method, .. } if method == "workspace/willRenameFiles")
                })
                .count()
                == 2
        });
    session.clock.wait_for_sleep(Duration::from_secs(30));
    assert!(late.as_mut().now_or_never().is_none());
    session.clock.advance(Duration::from_secs(30));
    assert!(answer(late).is_empty());
    session.fake.wait_for("a cancel", |records| {
        records
            .iter()
            .any(|record| record.is_notification("$/cancelRequest"))
    });
}

#[test]
fn file_operation_capabilities_are_advertised() {
    let session = Session::new(Script::default());
    let initialize = session
        .fake
        .records()
        .into_iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        initialize["capabilities"]["workspace"]["fileOperations"],
        json!({
            "dynamicRegistration": false,
            "didCreate": true,
            "willRename": true,
            "didRename": true,
            "didDelete": true,
        })
    );
}
