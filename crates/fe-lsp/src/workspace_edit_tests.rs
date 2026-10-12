//! Workspace edits: the client capabilities that invite them, decoding
//! both forms against open and closed files, what refuses one, and
//! `workspace/applyEdit` through the sink and back.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::task::Context;
use std::time::{Duration, Instant};

use fe_buffer::{Buffer, Origin};
use fe_text::Text;
use futures::FutureExt;
use lsp_types::request::ApplyWorkspaceEdit;
use lsp_types::{
    ApplyWorkspaceEditParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Position, PositionEncodingKind, TextDocumentIdentifier,
    TextDocumentItem, TextEdit, Url, VersionedTextDocumentIdentifier, WorkspaceEdit,
};
use serde_json::{Value, json};

use super::{APPLY_TIMEOUT, EditTarget, Edits, FileEdits, decode, decode_closed, not_applied};
use crate::document::{Note, RING, SentDocument};
use crate::fake::{Echo, FakeServer, InitializeReply, Record, Script, initialize_result};
use crate::server::Handed;
use crate::{
    Clock, LspError, LspEvent, LspManager, ManualClock, PositionEncoding, ServerId, ServerSpec,
    ServerStatus, SystemClock,
};

/// How long a test waits for anything before failing.
const WAIT: Duration = Duration::from_secs(10);

const ROOT: &str = "/fake/root";

/// A server on the fake choosing UTF-16, with a manual clock.
struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    server: ServerId,
    events: Receiver<LspEvent>,
}

impl Started {
    fn new() -> Self {
        let fake = FakeServer::new(Script {
            initialize: InitializeReply::Result(Box::new(initialize_result(Some(
                PositionEncodingKind::UTF16,
            )))),
            ..Script::default()
        });
        let clock = Arc::new(ManualClock::new());
        let (sender, events) = std::sync::mpsc::channel();
        let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), move |event| {
            let _ = sender.send(event);
        });
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        fake.wait_initialized();
        let s = Self {
            fake,
            clock,
            manager,
            server,
            events,
        };
        assert_eq!(
            s.next("status", |event| match event {
                LspEvent::Status { status, .. } => Some(status),
                _ => None,
            }),
            ServerStatus::Starting
        );
        s
    }

    /// Opens `buffer` as `name` under the root, edits it to `text`, and
    /// waits until the fake has that: the server's version is past its
    /// first.
    fn open(&mut self, buffer: &mut Buffer, name: &str, text: &str) -> Url {
        let path = PathBuf::from(ROOT).join(name);
        let uri = Url::from_file_path(&path).unwrap();
        let subscription = buffer.subscribe();
        self.manager
            .open_on(
                self.server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                "text",
            )
            .unwrap();
        self.fake
            .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        let len = buffer.text().len();
        buffer
            .edit([(0..len, text)], Origin::User)
            .expect("the edit changed something");
        self.manager.changed(buffer.id());
        self.fake
            .wait_for_document(&uri, "the edit", |r| r.is_some_and(|r| r.text == text));
        uri
    }

    /// Puts `text` at the start of `buffer`, open at `uri`, and waits
    /// until the fake has it.
    fn prepend(&self, buffer: &mut Buffer, uri: &Url, text: &str) {
        let want = format!("{text}{}", buffer.text());
        buffer
            .edit([(0..0, text)], Origin::User)
            .expect("the edit changed something");
        self.manager.changed(buffer.id());
        self.fake
            .wait_for_document(uri, "the edit", |r| r.is_some_and(|r| r.text == want));
    }

    /// The version the fake has of `uri`.
    fn version(&self, uri: &Url) -> i32 {
        self.fake.document(uri).unwrap().version
    }

    fn decode(&self, edit: Value) -> Result<Vec<FileEdits>, LspError> {
        let edit: WorkspaceEdit = serde_json::from_value(edit).unwrap();
        answer(self.manager.decode_workspace_edit(self.server, edit))
    }

    /// The next event `pick` takes, skipping others.
    fn next<T>(&self, what: &str, mut pick: impl FnMut(LspEvent) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => {
                    if let Some(picked) = pick(event) {
                        return picked;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no {what} in {WAIT:?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the manager has gone"),
            }
        }
    }

    /// Sends `workspace/applyEdit` from the fake on a thread of its own,
    /// as it blocks until answered.
    fn apply(&self, edit: Value) -> std::thread::JoinHandle<Value> {
        let fake = self.fake.clone();
        let params = ApplyWorkspaceEditParams {
            label: Some("Rename".into()),
            edit: serde_json::from_value(edit).unwrap(),
        };
        std::thread::spawn(move || fake.request::<ApplyWorkspaceEdit>(params).unwrap())
    }
}

/// Waits for `reply`; panics after [`WAIT`] of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = SystemClock.sleep(WAIT).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in {WAIT:?}"),
        }
    })
}

/// The LSP position of byte `offset` of `text`, in UTF-16, counted as the
/// protocol says, not with fe-lsp's converter.
fn position(text: &str, offset: usize) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
    Position::new(line as u32, column.encode_utf16().count() as u32)
}

/// A `TextEdit` replacing `range` of `text` with `new`, as JSON.
fn text_edit(text: &str, range: std::ops::Range<usize>, new: &str) -> Value {
    json!({
        "range": { "start": position(text, range.start), "end": position(text, range.end) },
        "newText": new,
    })
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(ROOT).join(name)
}

fn uri(name: &str) -> Url {
    Url::from_file_path(path(name)).unwrap()
}

/// Asserts `result` refused the edit, with a reason containing `why`.
fn assert_refused(result: Result<Vec<FileEdits>, LspError>, why: &str) {
    match result {
        Err(LspError::RefusedEdit(reason)) => {
            assert!(reason.contains(why), "{reason:?} doesn't say {why:?}");
        }
        other => panic!("not refused for {why:?}: {other:?}"),
    }
}

#[test]
fn capabilities_are_declared() {
    let s = Started::new();
    let init = (s.fake.records().into_iter())
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    let text_document = &init["capabilities"]["textDocument"];
    assert_eq!(
        text_document["rename"],
        json!({ "dynamicRegistration": false, "prepareSupport": true })
    );
    let action = &text_document["codeAction"];
    assert_eq!(action["dynamicRegistration"], json!(false));
    assert_eq!(
        action["codeActionLiteralSupport"]["codeActionKind"]["valueSet"],
        json!([
            "",
            "quickfix",
            "refactor",
            "refactor.extract",
            "refactor.inline",
            "refactor.rewrite",
            "source",
            "source.organizeImports",
            "source.fixAll",
        ])
    );
    assert_eq!(action["isPreferredSupport"], json!(true));
    assert_eq!(action["dataSupport"], json!(true));
    assert_eq!(action["resolveSupport"], json!({ "properties": ["edit"] }));
    for formatting in ["formatting", "rangeFormatting"] {
        assert_eq!(
            text_document[formatting],
            json!({ "dynamicRegistration": false })
        );
    }
    assert_eq!(
        text_document["signatureHelp"],
        json!({
            "dynamicRegistration": false,
            "signatureInformation": {
                "documentationFormat": ["markdown", "plaintext"],
                "parameterInformation": { "labelOffsetSupport": true },
                "activeParameterSupport": true,
            },
            "contextSupport": true,
        })
    );
    let workspace = &init["capabilities"]["workspace"];
    assert_eq!(workspace["applyEdit"], json!(true));
    assert_eq!(
        workspace["workspaceEdit"],
        json!({
            "documentChanges": true,
            "resourceOperations": [],
            "failureHandling": "abort",
        })
    );
    // What was there before is still there.
    assert_eq!(workspace["configuration"], json!(true));
    assert_eq!(workspace["diagnostics"], json!({ "refreshSupport": true }));
}

#[test]
fn decodes_changes_and_document_changes() {
    let mut s = Started::new();
    // An emoji ahead of the edits: two UTF-16 units, four bytes.
    let text = "let 🦀 = alpha;\nalpha.beta();\n";
    let mut buffer = Buffer::new("");
    let open = s.open(&mut buffer, "open.rs", text);
    let version = s.version(&open);
    let alpha = text.find("alpha").unwrap();
    let second = text.rfind("alpha").unwrap();
    // The closed file's text, which the host reads.
    let closed_text = "fn alpha() {}\n// 🦀 alpha\n";
    let closed_alpha = closed_text.find("alpha").unwrap();
    let closed_second = closed_text.rfind("alpha").unwrap();

    // Out of order, as a server may send them.
    let open_edits = json!([
        text_edit(text, second..second + 5, "gamma"),
        text_edit(text, alpha..alpha + 5, "gamma"),
    ]);
    let closed_edits = json!([
        text_edit(closed_text, closed_second..closed_second + 5, "gamma"),
        text_edit(closed_text, closed_alpha..closed_alpha + 5, "gamma"),
    ]);
    let want_open = FileEdits {
        path: path("open.rs"),
        target: EditTarget::Open {
            buffer: buffer.id(),
            version: buffer.version(),
            edits: vec![
                (alpha..alpha + 5, "gamma".into()),
                (second..second + 5, "gamma".into()),
            ],
        },
    };
    let closed_sorted: Vec<TextEdit> =
        serde_json::from_value(json!([closed_edits[1], closed_edits[0]])).unwrap();
    let want_closed = FileEdits {
        path: path("closed.rs"),
        target: EditTarget::Closed {
            edits: closed_sorted.clone(),
            encoding: PositionEncoding::Utf16,
        },
    };

    // `changes`: unversioned, files in path order.
    let changes = s
        .decode(json!({ "changes": {
            uri("open.rs").as_str(): open_edits,
            uri("closed.rs").as_str(): closed_edits,
        }}))
        .unwrap();
    assert_eq!(changes, vec![want_closed.clone(), want_open.clone()]);

    // `documentChanges`, versioned for the open file, in the server's
    // order; an annotated edit reads as the edit it carries.
    let annotated = {
        let mut edit = closed_edits[0].clone();
        edit["annotationId"] = json!("rename");
        edit
    };
    let document_changes = s
        .decode(json!({
            "documentChanges": [
                {
                    "textDocument": { "uri": uri("open.rs"), "version": version },
                    "edits": open_edits,
                },
                {
                    "textDocument": { "uri": uri("closed.rs"), "version": null },
                    "edits": [annotated, closed_edits[1]],
                },
            ],
            "changeAnnotations": { "rename": { "label": "Rename" } },
        }))
        .unwrap();
    assert_eq!(document_changes, vec![want_open, want_closed]);

    // The host places the closed file's edits in the text it reads.
    let decoded = decode_closed(
        &Text::new(closed_text),
        &closed_sorted,
        PositionEncoding::Utf16,
    )
    .unwrap();
    assert_eq!(
        decoded,
        vec![
            (closed_alpha..closed_alpha + 5, "gamma".to_owned()),
            (closed_second..closed_second + 5, "gamma".to_owned()),
        ]
    );
    // Overlapping, they're refused there too.
    let overlapping: Vec<TextEdit> = serde_json::from_value(json!([
        text_edit(closed_text, closed_alpha..closed_alpha + 5, "x"),
        text_edit(closed_text, closed_alpha + 2..closed_alpha + 3, "y"),
    ]))
    .unwrap();
    assert!(
        decode_closed(
            &Text::new(closed_text),
            &overlapping,
            PositionEncoding::Utf16
        )
        .is_err()
    );

    // Inserts at one place keep the server's order, and an edit ending
    // where the next starts doesn't overlap it.
    let joined = s
        .decode(json!({ "changes": { uri("open.rs").as_str(): [
            text_edit(text, alpha..alpha + 5, "x"),
            text_edit(text, alpha + 5..alpha + 5, "2"),
            text_edit(text, alpha..alpha, "1"),
            text_edit(text, alpha + 5..alpha + 5, "3"),
        ]}}))
        .unwrap();
    let EditTarget::Open { edits, .. } = &joined[0].target else {
        panic!("not open: {joined:?}");
    };
    assert_eq!(
        edits,
        &vec![
            (alpha..alpha, "1".to_owned()),
            (alpha..alpha + 5, "x".to_owned()),
            (alpha + 5..alpha + 5, "2".to_owned()),
            (alpha + 5..alpha + 5, "3".to_owned()),
        ]
    );
}

#[test]
fn refuses_bad_edits() {
    let mut s = Started::new();
    let text = "alpha beta gamma\n";
    let mut buffer = Buffer::new("");
    let open = s.open(&mut buffer, "open.rs", text);
    let version = s.version(&open);
    let fine = json!({
        "textDocument": { "uri": uri("open.rs"), "version": version },
        "edits": [text_edit(text, 0..5, "delta")],
    });
    // Alone, the good part decodes.
    assert!(s.decode(json!({ "documentChanges": [fine] })).is_ok());

    // A version the server doesn't have, even beside a good part.
    // The version before the server's, which the edit's text was older
    // than, alone or beside a good part.
    let stale = json!({
        "textDocument": { "uri": uri("open.rs"), "version": version - 1 },
        "edits": [text_edit(text, 6..10, "zeta")],
    });
    for edit in [json!([stale]), json!([fine, stale])] {
        assert_refused(s.decode(json!({ "documentChanges": edit })), "has changed");
    }
    // Beside an unversioned part for the file: the versioned part still
    // says which text it's for.
    let unversioned = json!({
        "textDocument": { "uri": uri("open.rs"), "version": null },
        "edits": [text_edit(text, 11..16, "eta")],
    });
    assert_refused(
        s.decode(json!({ "documentChanges": [unversioned, stale] })),
        "has changed",
    );
    assert!(
        s.decode(json!({ "documentChanges": [unversioned, fine] }))
            .is_ok()
    );
    // A version of a file the server doesn't have open.
    assert_refused(
        s.decode(json!({ "documentChanges": [fine, {
            "textDocument": { "uri": uri("closed.rs"), "version": 3 },
            "edits": [text_edit(text, 0..5, "delta")],
        }]})),
        "isn't open",
    );

    // Creating or renaming a file.
    assert_refused(
        s.decode(json!({ "documentChanges": [
            fine,
            { "kind": "create", "uri": uri("new.rs") },
        ]})),
        "creates",
    );
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "rename", "oldUri": uri("open.rs"), "newUri": uri("moved.rs") },
            fine,
        ]})),
        "renames",
    );

    // Overlapping edits in one file, open or closed, in one edit or across
    // two for the file.
    for name in ["open.rs", "closed.rs"] {
        assert_refused(
            s.decode(json!({ "changes": { uri(name).as_str(): [
                text_edit(text, 0..10, "x"),
                text_edit(text, 6..16, "y"),
            ]}})),
            "overlap",
        );
    }
    assert_refused(
        s.decode(json!({ "documentChanges": [fine, {
            "textDocument": { "uri": uri("open.rs"), "version": version },
            "edits": [text_edit(text, 2..8, "z")],
        }]})),
        "overlap",
    );

    // A version the server doesn't have, or of a file it doesn't have
    // open, refuses the edit even with no edits in it.
    for empty in [
        json!({ "textDocument": { "uri": uri("open.rs"), "version": version - 1 }, "edits": [] }),
        json!({ "textDocument": { "uri": uri("closed.rs"), "version": 3 }, "edits": [] }),
    ] {
        assert_refused(
            s.decode(json!({ "documentChanges": [fine, empty] })),
            "version",
        );
    }

    // A range ending before it starts, in an open file or a closed one,
    // even where swapping its ends would keep it apart from the others;
    // and in decode_closed.
    let reversed = |text: &str| {
        json!([
            text_edit(text, 0..2, "x"),
            {
                "range": { "start": position(text, 5), "end": position(text, 3) },
                "newText": "y",
            },
        ])
    };
    for name in ["open.rs", "closed.rs"] {
        assert_refused(
            s.decode(json!({ "changes": { uri(name).as_str(): reversed(text) } })),
            "before it starts",
        );
    }
    let reversed: Vec<TextEdit> = serde_json::from_value(reversed(text)).unwrap();
    assert!(
        decode_closed(&Text::new(text), &reversed, PositionEncoding::Utf16)
            .unwrap_err()
            .contains("before it starts")
    );

    // A URI that isn't a file.
    assert_refused(
        s.decode(json!({ "changes": {
            uri("open.rs").as_str(): [text_edit(text, 0..5, "delta")],
            "untitled:Untitled-1": [text_edit(text, 0..5, "delta")],
        }})),
        "isn't a file",
    );

    // A refused `workspace/applyEdit` is answered at once, with no event.
    let refused = s.apply(json!({ "documentChanges": [fine, stale] }));
    let reply = refused.join().unwrap();
    assert_eq!(reply["applied"], json!(false));
    assert!(
        (reply["failureReason"].as_str().unwrap()).contains("has changed"),
        "{reply}"
    );
    assert!(
        (s.events.try_iter()).all(|event| !matches!(event, LspEvent::ApplyEdit { .. })),
        "a refused edit reached the sink"
    );
}

#[test]
fn apply_edit_round_trip() {
    let mut s = Started::new();
    let text = "alpha beta\n";
    let mut buffer = Buffer::new("");
    let open = s.open(&mut buffer, "open.rs", text);
    let version = s.version(&open);
    let edit = json!({ "documentChanges": [{
        "textDocument": { "uri": open, "version": version },
        "edits": [text_edit(text, 0..5, "gamma")],
    }]});
    let want = vec![FileEdits {
        path: path("open.rs"),
        target: EditTarget::Open {
            buffer: buffer.id(),
            version: buffer.version(),
            edits: vec![(0..5, "gamma".into())],
        },
    }];
    let next_apply = |s: &Started| {
        s.next("ApplyEdit", |event| match event {
            LspEvent::ApplyEdit {
                server,
                label,
                edits,
                reply,
            } => Some((server, label, edits, reply)),
            _ => None,
        })
    };

    // Applied.
    let asked = s.apply(edit.clone());
    let (server, label, edits, reply) = next_apply(&s);
    assert_eq!(server, s.server);
    assert_eq!(label.as_deref(), Some("Rename"));
    assert_eq!(edits, want);
    assert!(!reply.is_answered());
    let again = reply.clone();
    reply.applied();
    assert!(again.is_answered());
    // Only the first answer counts.
    again.failed("too late");
    assert_eq!(asked.join().unwrap(), json!({ "applied": true }));

    // Failed, with the host's reason.
    let asked = s.apply(edit.clone());
    let (.., reply) = next_apply(&s);
    reply.failed("the buffer changed");
    assert_eq!(
        asked.join().unwrap(),
        json!({ "applied": false, "failureReason": "the buffer changed" })
    );

    // Dropped unanswered.
    let asked = s.apply(edit.clone());
    drop(next_apply(&s));
    assert_eq!(asked.join().unwrap()["applied"], json!(false));

    // Unanswered for 5 s on the clock, while the host still holds it.
    let asked = s.apply(edit.clone());
    let (.., reply) = next_apply(&s);
    s.clock.wait_for_sleep(APPLY_TIMEOUT);
    s.clock.advance(APPLY_TIMEOUT - Duration::from_millis(1));
    assert!(!asked.is_finished());
    s.clock.advance(Duration::from_millis(1));
    assert_eq!(
        asked.join().unwrap(),
        json!({ "applied": false, "failureReason": "not answered in time" })
    );
    // The timeout answered it: answering it late reaches nothing.
    assert!(reply.is_answered());
    reply.applied();

    // An edit with nothing in it is applied, with no event.
    let asked = s.apply(json!({ "changes": {} }));
    assert_eq!(asked.join().unwrap(), json!({ "applied": true }));
}

/// One timed out before the actor took it is answered not applied, and
/// never reaches the sink.
#[test]
fn apply_edit_timed_out_before_the_actor_is_dropped() {
    let edits = Edits::default();
    let clock = ManualClock::new();
    let params = ApplyWorkspaceEditParams {
        label: None,
        edit: WorkspaceEdit::default(),
    };
    let response = edits.ask(params, &clock);
    clock.advance(APPLY_TIMEOUT);
    assert_eq!(answer(response), not_applied("not answered in time"),);
    let waker = futures::task::noop_waker();
    assert!(edits.poll_take(&Context::from_waker(&waker)).is_pending());
}

/// The documents as `s`'s actor has them, each at its newest version.
fn documents(s: &Started) -> Vec<SentDocument> {
    answer(
        s.manager
            .request_with_documents::<Echo>(s.server, json!({})),
    )
    .unwrap()
    .1
    .documents
}

/// `uri`'s document in `documents`.
fn document(documents: Vec<SentDocument>, uri: &Url) -> SentDocument {
    documents
        .into_iter()
        .find(|document| document.uri == *uri)
        .unwrap()
}

fn did_open(sent: &SentDocument) -> Note {
    Note::Open(
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(
                sent.uri.clone(),
                "text".into(),
                sent.sent.lsp_version,
                sent.sent.text.to_string(),
            ),
        },
        Box::new(sent.clone()),
    )
}

fn did_change(sent: &SentDocument) -> Note {
    Note::Change(
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(
                sent.uri.clone(),
                sent.sent.lsp_version,
            ),
            content_changes: Vec::new(),
        },
        Box::new(sent.clone()),
    )
}

fn did_close(sent: &SentDocument) -> Note {
    Note::Close(
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier::new(sent.uri.clone()),
        },
        (sent.buffer, sent.identity),
    )
}

/// What the actor reads a `workspace/applyEdit` against ([`Handed`]): each
/// document as its latest note handed to async-lsp left it, whatever is
/// queued behind, even once the document's ring has moved past it.
/// (async-lsp's loop doesn't read while its own writes are stuck, so the
/// fake can't time a request into the gap where notes wait in the outbox:
/// this hands the notes the actor would, carrying the actor's own
/// snapshots.)
#[test]
fn apply_edit_reads_what_was_handed() {
    let mut s = Started::new();
    let text = "alpha beta\n";
    let mut a = Buffer::new("");
    let a_uri = s.open(&mut a, "a.rs", text);
    let opened = document(documents(&s), &a_uri);
    // More versions than the ring keeps, each as its `didChange` carries
    // it.
    let changes: Vec<SentDocument> = (0..RING + 2)
        .map(|_| {
            s.prepend(&mut a, &a_uri, "x");
            document(documents(&s), &a_uri)
        })
        .collect();
    let first = &changes[0].sent;
    let ring = answer(s.manager.ring(a.id()));
    assert!(!ring.iter().any(|&(lsp, _)| lsp == first.lsp_version));

    let decode = |handed: &Handed, edit: Value| {
        decode(
            serde_json::from_value(edit).unwrap(),
            &handed.documents(),
            PositionEncoding::Utf16,
        )
    };
    let gamma = |text: &str| json!([text_edit(text, 0..5, "gamma")]);
    let at = |version: i32| {
        json!({ "documentChanges": [{
            "textDocument": { "uri": a_uri, "version": version },
            "edits": gamma(text),
        }]})
    };
    let open_at = |version| EditTarget::Open {
        buffer: a.id(),
        version,
        edits: vec![(0..5, "gamma".into())],
    };

    // A drain's first `didChange` handed, the rest still queued: the
    // version handed is read, though gone from the ring; an unversioned
    // edit is read against it; the newer ones are refused.
    let mut handed = Handed::default();
    handed.hand(&did_open(&opened));
    handed.hand(&did_change(&changes[0]));
    assert_eq!(
        decode(&handed, at(first.lsp_version)).unwrap()[0].target,
        open_at(first.version)
    );
    assert_eq!(
        decode(
            &handed,
            json!({ "changes": { a_uri.as_str(): gamma(text) } })
        )
        .unwrap()[0]
            .target,
        open_at(first.version)
    );
    let old = changes.last().unwrap().clone();
    assert!(
        decode(&handed, at(old.sent.lsp_version))
            .unwrap_err()
            .contains("has changed")
    );

    // The rest handed, then a close with a reopen queued: the server has
    // no document, so a versioned edit is refused and an unversioned one
    // is for a closed file.
    for change in &changes[1..] {
        handed.hand(&did_change(change));
    }
    s.manager.close(a.id());
    let a_uri = s.open(&mut a, "a.rs", "reopened\n");
    let reopened = document(documents(&s), &a_uri);
    assert_ne!(reopened.identity, old.identity);
    handed.hand(&did_close(&old));
    assert!(
        decode(&handed, at(old.sent.lsp_version))
            .unwrap_err()
            .contains("isn't open")
    );
    assert!(matches!(
        decode(
            &handed,
            json!({ "changes": { a_uri.as_str(): gamma(text) } })
        )
        .unwrap()[0]
            .target,
        EditTarget::Closed { .. }
    ));
    // The reopen handed: its version.
    handed.hand(&did_open(&reopened));
    assert!(matches!(
        decode(&handed, at(reopened.sent.lsp_version)).unwrap()[0].target,
        EditTarget::Open { version, .. } if version == reopened.sent.version
    ));
    // A late close of the old incarnation doesn't close the new one.
    handed.hand(&did_close(&old));
    let documents = handed.documents();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].identity, reopened.identity);
}
