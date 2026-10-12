//! Rename: what `prepareRename` answers in each shape, a server without
//! `prepareProvider` or `renameProvider`, and a rename's edits for an open
//! file and a closed one.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{PrepareRenameRequest, Rename, Request};
use lsp_types::{OneOf, Position, PositionEncodingKind, RenameOptions, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::RenameTarget;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::workspace_edit::{EditTarget, FileEdits};
use crate::{LspError, LspEvent, LspManager, ManualClock, ServerSpec, SystemClock};

const WAIT: Duration = Duration::from_secs(10);
const ROOT: &str = "/fake/root";

/// A UTF-16 server on the fake with `rename_provider`, and a buffer open
/// on it as `a.rs`.
struct Started {
    fake: FakeServer,
    manager: LspManager,
    buffer: Buffer,
    uri: Url,
}

impl Started {
    fn new(rename_provider: Option<OneOf<bool, RenameOptions>>, text: &str) -> Self {
        let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
        init.capabilities = ServerCapabilities {
            rename_provider,
            ..init.capabilities
        };
        let fake = FakeServer::new(Script {
            initialize: InitializeReply::Result(Box::new(init)),
            ..Script::default()
        });
        let (sender, events) = std::sync::mpsc::channel::<LspEvent>();
        let mut manager = LspManager::with_clock(
            fake.launcher(),
            Arc::new(ManualClock::new()),
            move |event| {
                let _ = sender.send(event);
            },
        );
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        fake.wait_initialized();
        // The events aren't read; keep the channel open.
        std::mem::forget(events);

        let mut buffer = Buffer::new(text);
        let path = PathBuf::from(ROOT).join("a.rs");
        let uri = Url::from_file_path(&path).unwrap();
        let subscription = buffer.subscribe();
        manager
            .open_on(
                server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                "text",
            )
            .unwrap();
        fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        Self {
            fake,
            manager,
            buffer,
            uri,
        }
    }

    fn reply(&self, method: &str, reply: Reply) {
        self.fake.set_reply(method, Scripted::now(reply));
    }

    /// The requests for `method` the fake got.
    fn requests(&self, method: &str) -> Vec<Value> {
        (self.fake.records().into_iter())
            .filter_map(|record| match record {
                Record::Request {
                    method: m, params, ..
                } if m == method => Some(params),
                _ => None,
            })
            .collect()
    }

    fn prepare(&self, offset: usize) -> Result<Option<RenameTarget>, LspError> {
        let (buffer, version) = (self.buffer.id(), self.buffer.version());
        answer(self.manager.prepare_rename(buffer, version, offset))
    }

    fn rename(&self, offset: usize, new_name: &str) -> Result<Vec<FileEdits>, LspError> {
        let (buffer, version) = (self.buffer.id(), self.buffer.version());
        answer(
            self.manager
                .rename(buffer, version, offset, new_name.to_owned()),
        )
    }
}

fn prepares() -> Option<OneOf<bool, RenameOptions>> {
    Some(OneOf::Right(RenameOptions {
        prepare_provider: Some(true),
        work_done_progress_options: Default::default(),
    }))
}

/// Waits for `reply`; panics after [`WAIT`] of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = futures::FutureExt::fuse(crate::Clock::sleep(&SystemClock, WAIT));
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

fn range(text: &str, range: std::ops::Range<usize>) -> Value {
    json!({ "start": position(text, range.start), "end": position(text, range.end) })
}

#[test]
fn prepare() {
    // An emoji before the symbol: two UTF-16 units, four bytes.
    let text = "let 🦀 = alpha.beta();\n";
    let beta = text.find("beta").unwrap();
    let alpha = text.find("alpha").unwrap();
    let s = Started::new(prepares(), text);
    let method = PrepareRenameRequest::METHOD;

    // A bare range: its own text is the placeholder.
    s.reply(method, Reply::Result(range(text, beta..beta + 4)));
    assert_eq!(
        s.prepare(beta + 1).unwrap(),
        Some(RenameTarget::new(beta..beta + 4, "beta"))
    );
    // Asked at the offset, in the server's encoding.
    let sent = s.requests(method).pop().unwrap();
    assert_eq!(sent["textDocument"]["uri"], json!(s.uri));
    assert_eq!(sent["position"], json!(position(text, beta + 1)));

    // A range with the server's placeholder.
    s.reply(
        method,
        Reply::Result(json!({ "range": range(text, alpha..alpha + 5), "placeholder": "a!" })),
    );
    assert_eq!(
        s.prepare(alpha).unwrap(),
        Some(RenameTarget::new(alpha..alpha + 5, "a!"))
    );

    // `defaultBehavior`: the word at the offset.
    s.reply(method, Reply::Result(json!({ "defaultBehavior": true })));
    assert_eq!(
        s.prepare(beta + 2).unwrap(),
        Some(RenameTarget::new(beta..beta + 4, "beta"))
    );

    // Nothing renameable there.
    s.reply(method, Reply::Result(Value::Null));
    assert_eq!(s.prepare(beta).unwrap(), None);

    // A refusal keeps its message.
    s.reply(
        method,
        Reply::Error {
            code: -32602,
            message: "cannot rename this".into(),
        },
    );
    match s.prepare(beta) {
        Err(LspError::Server { message, .. }) => assert_eq!(message, "cannot rename this"),
        other => panic!("not the server's error: {other:?}"),
    }

    // A server that doesn't prepare: the word at the offset, no request.
    let s = Started::new(Some(OneOf::Left(true)), text);
    assert_eq!(
        s.prepare(alpha + 3).unwrap(),
        Some(RenameTarget::new(alpha..alpha + 5, "alpha"))
    );
    // Beside the word is still the word; in the gap is none.
    assert_eq!(
        s.prepare(alpha + 5).unwrap(),
        Some(RenameTarget::new(alpha..alpha + 5, "alpha"))
    );
    assert_eq!(s.prepare(text.find(" =").unwrap() + 1).unwrap(), None);
    assert!(s.requests(method).is_empty());

    // A server that can't rename.
    for provider in [None, Some(OneOf::Left(false))] {
        let s = Started::new(provider, text);
        assert!(matches!(
            s.prepare(beta),
            Err(LspError::Unavailable(why)) if why == "rename"
        ));
        assert!(s.requests(method).is_empty());
    }
}

#[test]
fn rename_edits() {
    let text = "let 🦀 = alpha.beta();\n";
    let beta = text.find("beta").unwrap();
    let s = Started::new(prepares(), text);
    let closed = "fn beta() {}\n";
    let closed_uri = Url::from_file_path(PathBuf::from(ROOT).join("b.rs")).unwrap();
    s.reply(
        Rename::METHOD,
        Reply::Result(json!({ "changes": {
            s.uri.as_str(): [{ "range": range(text, beta..beta + 4), "newText": "gamma" }],
            closed_uri.as_str(): [{ "range": range(closed, 3..7), "newText": "gamma" }],
        }})),
    );
    let files = s.rename(beta + 1, "gamma").unwrap();

    let sent = s.requests(Rename::METHOD).pop().unwrap();
    assert_eq!(sent["newName"], json!("gamma"));
    assert_eq!(sent["textDocument"]["uri"], json!(s.uri));
    assert_eq!(sent["position"], json!(position(text, beta + 1)));

    assert_eq!(files.len(), 2);
    let open = files
        .iter()
        .find(|file| file.path.ends_with("a.rs"))
        .unwrap();
    assert_eq!(
        open.target,
        EditTarget::Open {
            buffer: s.buffer.id(),
            version: s.buffer.version(),
            edits: vec![(beta..beta + 4, "gamma".into())],
        }
    );
    let other = files
        .iter()
        .find(|file| file.path.ends_with("b.rs"))
        .unwrap();
    let EditTarget::Closed { edits, .. } = &other.target else {
        panic!("not closed: {other:?}");
    };
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "gamma");

    // A `null` reply is no edits.
    s.reply(Rename::METHOD, Reply::Result(Value::Null));
    assert_eq!(s.rename(beta, "gamma").unwrap(), Vec::new());

    // A server that can't rename: no request.
    for provider in [None, Some(OneOf::Left(false))] {
        let s = Started::new(provider, text);
        assert!(matches!(
            s.rename(beta, "gamma"),
            Err(LspError::Unavailable(why)) if why == "rename"
        ));
        assert!(s.requests(Rename::METHOD).is_empty());
    }
}
