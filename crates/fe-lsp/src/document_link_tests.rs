//! Document links on the fake server: decoding, resolving, and the
//! capabilities that ask for them.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use lsp_types::{DocumentLinkOptions, PositionEncodingKind};
use serde_json::{Value, json};

use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{DocumentLink, LspError, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = "/fake/root";
const WAIT: Duration = Duration::from_secs(10);
const LINKS: &str = "textDocument/documentLink";
const RESOLVE: &str = "documentLink/resolve";

/// Two rows, each an emoji (two UTF-16 units, four bytes) before a link.
const TEXT: &str = "🦀 // see https://a.example/x\n🦀 // and ./b.rs\n";

struct Started {
    fake: FakeServer,
    manager: LspManager,
    buffer: Buffer,
}

impl Started {
    /// `provider`: whether it has a `documentLinkProvider`, and then
    /// whether that resolves.
    fn new(provider: Option<bool>) -> Self {
        let mut result = initialize_result(Some(PositionEncodingKind::UTF16));
        result.capabilities.document_link_provider = provider.map(|resolves| DocumentLinkOptions {
            resolve_provider: Some(resolves),
            work_done_progress_options: Default::default(),
        });
        let fake = FakeServer::new(Script {
            initialize: InitializeReply::Result(Box::new(result)),
            ..Script::default()
        });
        let clock = Arc::new(ManualClock::new());
        let mut manager = LspManager::with_clock(fake.launcher(), clock, |_| {});
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        fake.wait_initialized();
        let mut buffer = Buffer::new(TEXT);
        let path = PathBuf::from(ROOT).join("a.rs");
        let uri = lsp_types::Url::from_file_path(&path).unwrap();
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
        }
    }

    fn reply(&self, method: &str, reply: Value) {
        self.fake
            .set_reply(method, Scripted::now(Reply::Result(reply)));
    }

    fn ask(&self) -> Option<Vec<DocumentLink>> {
        answer(
            self.manager
                .document_links(self.buffer.id(), self.buffer.version()),
        )
        .unwrap()
    }

    fn resolve(&self, link: DocumentLink) -> Result<DocumentLink, LspError> {
        answer(
            self.manager
                .resolve_document_link(self.buffer.id(), self.buffer.version(), link),
        )
    }

    fn sent(&self, method: &str) -> Vec<Value> {
        (self.fake.records().into_iter())
            .filter_map(|record| match record {
                Record::Request {
                    method: m, params, ..
                } if m == method => Some(params),
                _ => None,
            })
            .collect()
    }
}

/// Waits for `reply`; panics after [`WAIT`] of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    use futures::FutureExt;
    let reply = reply.fuse();
    let timeout = crate::Clock::sleep(&SystemClock, WAIT).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in {WAIT:?}"),
        }
    })
}

/// The LSP position of byte `offset` of `TEXT`, in UTF-16.
fn position(offset: usize) -> Value {
    let before = &TEXT[..offset];
    let line = before.matches('\n').count();
    let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
    json!({ "line": line, "character": column.encode_utf16().count() })
}

/// The LSP range of `needle` in `TEXT`, in UTF-16.
fn range_of(needle: &str) -> Value {
    let bytes = bytes_of(needle);
    json!({ "start": position(bytes.start), "end": position(bytes.end) })
}

fn bytes_of(needle: &str) -> std::ops::Range<usize> {
    let start = TEXT.find(needle).unwrap();
    start..start + needle.len()
}

#[test]
fn links_decode_to_bytes() {
    let s = Started::new(Some(false));
    // The later row first.
    s.reply(
        LINKS,
        json!([
            { "range": range_of("./b.rs"), "target": "file:///fake/root/b.rs", "data": 2 },
            {
                "range": range_of("https://a.example/x"),
                "target": "https://a.example/x",
                "tooltip": "Follow link",
            },
        ]),
    );
    let links = s.ask().unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].range, bytes_of("https://a.example/x"));
    assert_eq!(links[0].target.as_deref(), Some("https://a.example/x"));
    assert_eq!(links[0].tooltip.as_deref(), Some("Follow link"));
    assert_eq!(links[1].range, bytes_of("./b.rs"));
    assert_eq!(links[1].target.as_deref(), Some("file:///fake/root/b.rs"));
    assert_eq!(links[1].data, Some(json!(2)));
    let sent = s.sent(LINKS);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["textDocument"]["uri"], "file:///fake/root/a.rs");
}

#[test]
fn no_provider_sends_nothing() {
    let s = Started::new(None);
    s.reply(LINKS, json!([{ "range": range_of("./b.rs") }]));
    assert_eq!(s.ask(), None);
    assert!(s.sent(LINKS).is_empty());
}

#[test]
fn empty_ranges_are_dropped() {
    let s = Started::new(Some(false));
    let at = position(bytes_of("./b.rs").start);
    s.reply(
        LINKS,
        json!([
            { "range": { "start": at, "end": at }, "target": "file:///fake/root/b.rs" },
            { "range": range_of("./b.rs"), "target": "file:///fake/root/b.rs" },
        ]),
    );
    let links = s.ask().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].range, bytes_of("./b.rs"));
}

#[test]
fn resolve_fills_the_target() {
    let s = Started::new(Some(true));
    s.reply(
        LINKS,
        json!([{ "range": range_of("./b.rs"), "data": { "id": 7 } }]),
    );
    let link = s.ask().unwrap().remove(0);
    assert_eq!(link.target, None);
    s.reply(
        RESOLVE,
        json!({
            "range": range_of("./b.rs"),
            "target": "file:///fake/root/b.rs",
            "data": { "id": 7 },
        }),
    );
    let resolved = s.resolve(link).unwrap();
    assert_eq!(resolved.range, bytes_of("./b.rs"));
    assert_eq!(resolved.target.as_deref(), Some("file:///fake/root/b.rs"));
    // The link went back as the server gave it, data and all.
    let sent = s.sent(RESOLVE);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["range"], range_of("./b.rs"));
    assert_eq!(sent[0]["data"], json!({ "id": 7 }));
    assert!(sent[0].get("target").is_none());

    // One with its target is answered as it is.
    let again = s.resolve(resolved.clone()).unwrap();
    assert_eq!(again, resolved);
    assert_eq!(s.sent(RESOLVE).len(), 1);
}

#[test]
fn resolve_without_provider_is_unavailable() {
    let s = Started::new(Some(false));
    s.reply(LINKS, json!([{ "range": range_of("./b.rs") }]));
    let link = s.ask().unwrap().remove(0);
    s.reply(
        RESOLVE,
        json!({ "range": range_of("./b.rs"), "target": "file:///x" }),
    );
    assert!(matches!(
        s.resolve(link.clone()),
        Err(LspError::Unavailable(why)) if why == RESOLVE
    ));
    assert!(s.sent(RESOLVE).is_empty());

    let s = Started::new(None);
    assert!(matches!(s.resolve(link), Err(LspError::Unavailable(_))));
    assert!(s.sent(RESOLVE).is_empty());
}

#[test]
fn targets_name_their_scheme_and_file() {
    let link = |target: &str| DocumentLink::new(0..1, Some(target.to_owned()));
    let file = link("file:///fake/root/b%20c.rs#L12,5");
    assert_eq!(file.scheme().as_deref(), Some("file"));
    assert_eq!(
        file.file_target(),
        Some((PathBuf::from("/fake/root/b c.rs"), Some("L12,5".to_owned())))
    );
    assert_eq!(link("file:///x").file_target(), Some(("/x".into(), None)));
    let web = link("HTTPS://a.example/");
    assert_eq!(web.scheme().as_deref(), Some("https"));
    assert_eq!(web.file_target(), None);
    assert_eq!(link("not a uri").scheme(), None);
    assert_eq!(DocumentLink::new(0..1, None).scheme(), None);
}
