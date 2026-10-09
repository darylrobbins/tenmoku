//! `LspNavigation` (docs/decisions/lsp-client.md §5): routing by readiness,
//! the empty-answer rule, stale and cancelled requests, offsets in each kind
//! of file, references and workspace symbols. The fake stands in for
//! rust-analyzer (a host spec of that name, so it gets Rust files), over a
//! real project folder, so search can answer and unopened files be read.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use fe_buffer::{Buffer, Origin};
use fe_config::settings::LspSettings;
use fe_nav::{
    Definitions, FileReferences, FileSymbols, Location, NavError, NavRequest, Navigation,
    NavigationProvider, OutlineKind, Precision, Readiness, ReferencesSummary, SearchNavigation,
    Symbol, SymbolIndex, Tier, WorkspaceSymbol,
};
use lsp_types::{NumberOrString, PositionEncodingKind, Url};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{Events, WAIT, manager};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::readiness::SETTLE;
use crate::router::{ServerStatusNotification, ServerStatusParams};
use crate::{
    LspManager, LspNavigation, ManualClock, ReadinessMode, ServerId, ServerSpec, ServerStatus,
};

/// The request's file: `helper` defined on row 0 and called on row 3.
const MAIN: &str = "fn helper() {}\n\nfn main() {\n    helper();\n}\n";
/// Another file, opened too.
const OTHER: &str = "pub fn other() {}\n";
/// A file never opened: CRLF, with an emoji before the name on its row.
const FAR: &str = "// 😀 far\r\nlet 😀 = 1; fn far() {}\r\n";
/// [`FAR`]'s row 1.
const FAR_ROW: &str = "let 😀 = 1; fn far() {}";

/// Search, counting the requests it's asked.
struct Counted {
    search: SearchNavigation,
    asked: Arc<AtomicUsize>,
}

impl NavigationProvider for Counted {
    fn name(&self) -> &str {
        self.search.name()
    }

    fn precision(&self) -> Precision {
        self.search.precision()
    }

    fn readiness(&self, language: Option<&str>) -> Readiness {
        self.search.readiness(language)
    }

    fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.search.definitions(req, cancel)
    }

    fn references(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.search.references(req, cancel, sink)
    }

    fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.search.workspace_symbols(query, limit, cancel)
    }
}

/// A project with [`MAIN`] open on the fake, and a [`Navigation`] holding
/// search at 0 and the manager's [`LspNavigation`] at 100.
struct Nav {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    /// The fake's clock, for its scripted delays.
    fake_clock: Arc<ManualClock>,
    manager: Arc<Mutex<LspManager>>,
    events: Events,
    server: ServerId,
    navigation: Navigation,
    lsp: Arc<LspNavigation>,
    /// How often search was asked.
    searched: Arc<AtomicUsize>,
    root: PathBuf,
    main: Buffer,
    _dir: TempDir,
}

impl Nav {
    fn new(readiness: ReadinessMode) -> Self {
        Self::with(readiness, Script::default(), None)
    }

    /// With the fake's `script`, and documents over `max_bytes` (if any)
    /// left unsynced.
    fn with(readiness: ReadinessMode, script: Script, max_bytes: Option<usize>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        for (name, text) in [
            ("src/main.rs", MAIN),
            ("src/other.rs", OTHER),
            ("src/far.rs", FAR),
        ] {
            std::fs::write(root.join(name), text).unwrap();
        }
        let fake_clock = Arc::new(ManualClock::new());
        let fake = FakeServer::with_clock(script, fake_clock.clone());
        let clock = Arc::new(ManualClock::new());
        let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
        manager.register_spec(ServerSpec {
            languages: vec!["rust".into()],
            readiness,
            ..ServerSpec::new("rust-analyzer", "fake-server")
        });
        if let Some(bytes) = max_bytes {
            manager.set_max_file_bytes(bytes);
        }
        let mut main = Buffer::new(MAIN);
        let server = open(&mut manager, &root, &mut main, "src/main.rs").unwrap();
        assert_eq!(events.status(server), ServerStatus::Starting);
        fake.wait_initialized();
        let manager = Arc::new(Mutex::new(manager));
        let lsp = Arc::new(LspNavigation::new(Arc::downgrade(&manager), &root));
        let searched = Arc::new(AtomicUsize::new(0));
        let index = Arc::new(RwLock::new(SymbolIndex::new(&root)));
        let search = Counted {
            search: SearchNavigation::new(&root, index).with_budget(WAIT),
            asked: searched.clone(),
        };
        let navigation = Navigation::new();
        navigation.add(Arc::new(search), 0);
        navigation.add(lsp.clone(), 100);
        Self {
            fake,
            clock,
            fake_clock,
            manager,
            events,
            server,
            navigation,
            lsp,
            searched,
            root,
            main,
            _dir: dir,
        }
    }

    /// Ready, by the mode's rule.
    fn ready(readiness: ReadinessMode) -> Self {
        let s = Self::new(readiness);
        match readiness {
            ReadinessMode::Progress => s.settle(),
            ReadinessMode::ServerStatus => s.status(true),
        }
        assert_eq!(s.events.status(s.server), ServerStatus::Ready);
        s
    }

    /// Lets `progress` mode's quiet period pass.
    fn settle(&self) {
        self.clock.wait_for_sleep(SETTLE);
        self.clock.advance(SETTLE);
    }

    /// Sends a `serverStatus`, and returns once the client has read it.
    fn status(&self, quiescent: bool) {
        self.fake
            .notify::<ServerStatusNotification>(ServerStatusParams {
                health: "ok".into(),
                quiescent,
                message: None,
            });
        // A request goes out behind it, and is answered after it's read.
        let echo = self
            .manager
            .lock()
            .unwrap()
            .request::<crate::fake::Echo>(self.server, json!({}));
        super::answer(echo).unwrap();
    }

    /// The server's next status but `Starting`, `Indexing` and `Ready`.
    fn settled_status(&self) -> ServerStatus {
        loop {
            match self.events.status(self.server) {
                ServerStatus::Starting | ServerStatus::Ready | ServerStatus::Indexing { .. } => {}
                status => return status,
            }
        }
    }

    fn uri(&self, rel: &str) -> Url {
        Url::from_file_path(self.root.join(rel)).unwrap()
    }

    /// A request with the caret on `helper` in `main`'s row 3, at its
    /// current version.
    fn request(&self) -> NavRequest {
        request(&self.root, &self.main)
    }

    fn reply(&self, method: &str, value: Value) {
        self.fake
            .set_reply(method, Scripted::now(Reply::Result(value)));
    }

    fn definitions(&self, req: &NavRequest) -> fe_nav::Answer<Definitions> {
        let cancel = AtomicBool::new(false);
        self.navigation.definitions(req, &cancel).unwrap()
    }

    fn readiness(&self) -> Readiness {
        self.lsp.readiness(Some("rust"))
    }

    /// Asserts search answered `req`, after the server failed to.
    fn search_answers(&self, req: &NavRequest, why: &str) {
        let before = self.searched.load(Ordering::SeqCst);
        let answer = self.definitions(req);
        assert_eq!(
            (answer.provider.as_str(), answer.precision),
            (SearchNavigation::NAME, Precision::Search),
            "{why}"
        );
        assert_eq!(self.searched.load(Ordering::SeqCst), before + 1, "{why}");
    }
}

/// Opens `buffer` at `rel` under `root` as Rust.
fn open(manager: &mut LspManager, root: &Path, buffer: &mut Buffer, rel: &str) -> Option<ServerId> {
    let subscription = buffer.subscribe();
    let (text, version) = (buffer.text().clone(), buffer.version());
    let path = root.join(rel);
    (manager.open(
        Some(root),
        buffer.id(),
        text,
        version,
        subscription,
        &path,
        "rust",
    ))
    .unwrap()
}

fn request(root: &Path, main: &Buffer) -> NavRequest {
    NavRequest {
        root: root.to_owned(),
        rel: PathBuf::from("src/main.rs"),
        language: Some("rust"),
        text: main.text().clone(),
        offset: MAIN.find("helper();").unwrap() + 2,
        buffer: Some((main.id(), main.version())),
    }
}

/// An LSP range on one row.
fn range(row: u32, start: usize, end: usize) -> Value {
    json!({
        "start": { "line": row, "character": start },
        "end": { "line": row, "character": end },
    })
}

fn location(uri: &Url, range: Value) -> Value {
    json!({ "uri": uri, "range": range })
}

/// Where `helper` is defined in [`MAIN`], as the fake answers it.
fn helper_definition(s: &Nav) -> Value {
    json!([location(&s.uri("src/main.rs"), range(0, 3, 9))])
}

/// A preview's one range.
fn one(range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
    vec![range]
}

/// The bytes of `word` in `text`.
fn bytes_of(text: &str, word: &str) -> std::ops::Range<usize> {
    let start = text.find(word).unwrap();
    start..start + word.len()
}

#[test]
fn routing_follows_readiness() {
    let s = Nav::new(ReadinessMode::Progress);
    s.reply("textDocument/definition", helper_definition(&s));
    let req = s.request();

    // Starting: search answers.
    assert_eq!(s.readiness(), Readiness::NotYet);
    let answer = s.definitions(&req);
    assert_eq!(answer.provider, SearchNavigation::NAME);
    assert_eq!(answer.precision, Precision::Search);
    assert_eq!(s.searched.load(Ordering::SeqCst), 1);

    // Ready: the fake's exact location, and search isn't asked.
    s.settle();
    assert_eq!(s.events.status(s.server), ServerStatus::Ready);
    assert_eq!(s.readiness(), Readiness::Ready);
    let answer = s.definitions(&req);
    assert_eq!(answer.provider, LspNavigation::NAME);
    assert_eq!(answer.precision, Precision::Exact);
    let [found] = &answer.value.items[..] else {
        panic!("{:?}", answer.value);
    };
    assert_eq!(found.location.range, 3..9);
    assert_eq!(found.location.rel, Path::new("src/main.rs"));
    assert_eq!(found.name, "helper");
    assert!(answer.value.unambiguous);
    assert_eq!(s.searched.load(Ordering::SeqCst), 1);

    // Restarting after a crash: not yet.
    s.fake.crash();
    assert_eq!(s.settled_status(), ServerStatus::Restarting(1));
    assert_eq!(s.readiness(), Readiness::NotYet);

    // Stopped: unsupported.
    s.manager.lock().unwrap().stop(s.server);
    assert_eq!(s.settled_status(), ServerStatus::Stopped(None));
    assert_eq!(s.readiness(), Readiness::Unsupported);

    // A language no server lists.
    assert_eq!(s.lsp.readiness(Some("markdown")), Readiness::Unsupported);

    // LSP turned off, with the server ready.
    let s = Nav::ready(ReadinessMode::Progress);
    assert_eq!(s.readiness(), Readiness::Ready);
    s.manager.lock().unwrap().update_settings(&LspSettings {
        enabled: false,
        ..LspSettings::default()
    });
    assert_eq!(s.readiness(), Readiness::Unsupported);
}

#[test]
fn empty_answers_and_errors() {
    let unavailable = |s: &Nav, req: &NavRequest, why: &str| {
        let answer = s.lsp.definitions(req, &AtomicBool::new(false));
        assert!(
            matches!(answer, Err(NavError::Unavailable(_))),
            "{why}: {answer:?}"
        );
        s.search_answers(req, why);
    };

    // Empty, from a progress-mode server: it can't say it's done.
    let s = Nav::ready(ReadinessMode::Progress);
    s.reply("textDocument/definition", Value::Null);
    unavailable(&s, &s.request(), "progress mode");

    // From a server-status server, not quiescent since it was ready.
    let s = Nav::ready(ReadinessMode::ServerStatus);
    s.status(false);
    s.reply("textDocument/definition", json!([]));
    unavailable(&s, &s.request(), "quiescent false");

    // Quiescent: final, and search isn't asked.
    s.status(true);
    let answer = s.definitions(&s.request());
    assert_eq!(answer.provider, LspNavigation::NAME);
    assert_eq!(answer.value.items, []);
    assert_eq!(answer.value.word.as_deref(), Some("helper"));
    assert_eq!(s.searched.load(Ordering::SeqCst), 1);

    // ContentModified.
    s.fake.set_reply(
        "textDocument/definition",
        Scripted::now(Reply::Error {
            code: -32801,
            message: "content modified".into(),
        }),
    );
    unavailable(&s, &s.request(), "ContentModified");

    // A file of no language: no server's.
    let req = NavRequest {
        language: None,
        ..s.request()
    };
    let answer = s.lsp.definitions(&req, &AtomicBool::new(false));
    assert_eq!(answer, Err(NavError::Unsupported));

    // No buffer named.
    let req = NavRequest {
        buffer: None,
        ..s.request()
    };
    unavailable(&s, &req, "no buffer");

    // A timeout, on the manager's clock.
    s.fake
        .set_reply("textDocument/definition", Scripted::now(Reply::Never));
    let navigation = &s.navigation;
    let req = s.request();
    thread::scope(|scope| {
        let asked = scope.spawn(|| navigation.definitions(&req, &AtomicBool::new(false)));
        let timeout = Duration::from_millis(LspSettings::default().timeouts.definition);
        s.clock.wait_for_sleep(timeout);
        s.clock.advance(timeout);
        let answer = asked.join().unwrap().unwrap();
        assert_eq!(answer.provider, SearchNavigation::NAME, "timeout");
    });

    // The server going down while it answers.
    let asked = |records: &[Record]| {
        (records.iter())
            .filter(|record| record.method() == "textDocument/definition")
            .count()
    };
    let before = asked(&s.fake.records());
    thread::scope(|scope| {
        let answer = scope.spawn(|| navigation.definitions(&req, &AtomicBool::new(false)));
        s.fake
            .wait_for("the definition request", |records| asked(records) > before);
        s.fake.crash();
        let answer = answer.join().unwrap().unwrap();
        assert_eq!(answer.provider, SearchNavigation::NAME, "ServerDown");
    });

    // A document over the size limit isn't synced.
    let s = Nav::with(ReadinessMode::Progress, Script::default(), Some(8));
    s.settle();
    assert_eq!(s.events.status(s.server), ServerStatus::Ready);
    s.reply("textDocument/definition", helper_definition(&s));
    unavailable(&s, &s.request(), "too large");
}

#[test]
fn stale_and_cancelled_requests() {
    let mut s = Nav::ready(ReadinessMode::Progress);
    s.reply("textDocument/definition", helper_definition(&s));

    // Made on a version the buffer has since moved past: the server isn't
    // asked, and search answers on the request's text.
    let stale = s.request();
    s.main.edit([(0..0, "// moved\n")], Origin::User).unwrap();
    assert_eq!(
        s.lsp.definitions(&stale, &AtomicBool::new(false)),
        Err(NavError::Unavailable("buffer changed".into()))
    );
    let answer = s.definitions(&stale);
    assert_eq!(answer.provider, SearchNavigation::NAME);
    let [found] = &answer.value.items[..] else {
        panic!("{:?}", answer.value);
    };
    assert_eq!(found.location.range, bytes_of(MAIN, "helper"));
    assert_eq!(found.location.rel, Path::new("src/main.rs"));

    // Cancelled while the fake holds its reply.
    s.fake
        .set_reply("textDocument/definition", Scripted::now(Reply::Never));
    let req = s.request();
    let cancel = AtomicBool::new(false);
    let sent = |records: &[Record]| {
        records.iter().rev().find_map(|record| match record {
            Record::Request { id, method, .. } if method == "textDocument/definition" => {
                Some(id.clone())
            }
            _ => None,
        })
    };
    let before = sent(&s.fake.records());
    let lsp = &s.lsp;
    let id = thread::scope(|scope| {
        let asked = scope.spawn(|| {
            let answer = lsp.definitions(&req, &cancel);
            (answer, Instant::now())
        });
        let records = s.fake.wait_for("the held request", |records| {
            sent(records).is_some() && sent(records) != before
        });
        let id = sent(&records).unwrap();
        let cancelled = Instant::now();
        cancel.store(true, Ordering::SeqCst);
        let (answer, answered) = asked.join().unwrap();
        assert_eq!(answer, Err(NavError::Cancelled));
        let took = answered - cancelled;
        assert!(took < Duration::from_millis(50), "answered after {took:?}");
        id
    });
    let NumberOrString::Number(id) = id else {
        panic!("{id:?}");
    };
    s.fake.wait_for("$/cancelRequest", |records| {
        records.iter().any(|record| match record {
            Record::Notification { method, params } => {
                method == "$/cancelRequest" && params["id"] == json!(id)
            }
            _ => false,
        })
    });
}

/// A link to `selection`, in an item spanning the rest of its row.
fn link(uri: &Url, row: u32, selection: std::ops::Range<usize>) -> Value {
    json!({
        "targetUri": uri,
        "targetRange": range(row, 0, selection.end + 4),
        "targetSelectionRange": range(row, selection.start, selection.end),
    })
}

#[test]
fn offsets_by_file() {
    let mut answers = Vec::new();
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let utf16 = encoding == PositionEncodingKind::UTF16;
        let script = Script {
            initialize: InitializeReply::Result(Box::new(initialize_result(Some(
                encoding.clone(),
            )))),
            ..Script::default()
        };
        let s = Nav::with(ReadinessMode::Progress, script, None);
        s.settle();
        assert_eq!(s.events.status(s.server), ServerStatus::Ready);

        // `other.rs` is open, and edited since the server last had it.
        let mut other = Buffer::new(OTHER);
        let sent = other.version();
        let other_uri = s.uri("src/other.rs");
        open(
            &mut s.manager.lock().unwrap(),
            &s.root,
            &mut other,
            "src/other.rs",
        );
        s.fake
            .wait_for_document(&other_uri, "didOpen", |r| r.is_some());
        other.edit([(0..0, "// edited\n")], Origin::User).unwrap();

        // `far.rs` isn't open: its column counts the emoji before it.
        let far = bytes_of(FAR_ROW, "far");
        let column = |bytes: usize| match utf16 {
            true => FAR_ROW[..bytes].encode_utf16().count(),
            false => bytes,
        };
        let far_columns = column(far.start)..column(far.end);
        let (main_uri, far_uri) = (s.uri("src/main.rs"), s.uri("src/far.rs"));
        let other_name = bytes_of(OTHER, "other");
        let as_locations = json!([
            location(&main_uri, range(0, 3, 9)),
            location(&other_uri, range(0, other_name.start, other_name.end)),
            location(&far_uri, range(1, far_columns.start, far_columns.end)),
        ]);
        let as_links = json!([
            link(&main_uri, 0, 3..9),
            link(&other_uri, 0, other_name.clone()),
            link(&far_uri, 1, far_columns.clone()),
        ]);
        let req = s.request();
        let far_text = FAR.replace("\r\n", "\n");
        let far_at = far_text.find(FAR_ROW).unwrap();
        let expected = vec![
            (
                Location {
                    rel: "src/main.rs".into(),
                    range: 3..9,
                    row: 0,
                    snapshot: req.buffer,
                },
                Tier::SameFile,
            ),
            (
                Location {
                    rel: "src/other.rs".into(),
                    range: other_name,
                    row: 0,
                    snapshot: Some((other.id(), sent)),
                },
                Tier::Project,
            ),
            (
                Location {
                    rel: "src/far.rs".into(),
                    range: far_at + far.start..far_at + far.end,
                    row: 1,
                    snapshot: None,
                },
                Tier::Project,
            ),
        ];
        for reply in [as_locations, as_links] {
            s.reply("textDocument/definition", reply);
            let answer = s.definitions(&req);
            assert_eq!(answer.provider, LspNavigation::NAME);
            let found: Vec<_> = (answer.value.items.iter())
                .map(|item| (item.location.clone(), item.tier))
                .collect();
            assert_eq!(found, expected, "{encoding:?}");
            let names: Vec<_> = answer
                .value
                .items
                .iter()
                .map(|item| &item.name[..])
                .collect();
            assert_eq!(names, ["helper", "other", "far"]);
            assert!(answer.value.items.iter().all(|item| item.kind.is_none()));
            assert!(!answer.value.unambiguous);
        }
        // The same bytes whatever the encoding (the buffers differ).
        answers.push(
            (expected.into_iter())
                .map(|(location, tier)| (location.rel, location.range, location.row, tier))
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(answers[0], answers[1]);
}

#[test]
fn references_and_symbols() {
    let s = Nav::ready(ReadinessMode::ServerStatus);
    let (main, far, other) = (
        s.uri("src/main.rs"),
        s.uri("src/far.rs"),
        s.uri("src/other.rs"),
    );
    let mut opened = Buffer::new(OTHER);
    open(
        &mut s.manager.lock().unwrap(),
        &s.root,
        &mut opened,
        "src/other.rs",
    );
    s.fake.wait_for_document(&other, "didOpen", |r| r.is_some());
    let far_name = bytes_of(FAR_ROW, "far");
    s.reply(
        "textDocument/references",
        json!([
            location(&other, range(0, 7, 12)),
            location(&main, range(3, 4, 10)),
            location(&far, range(1, far_name.start, far_name.end)),
            location(&main, range(0, 3, 9)),
        ]),
    );
    s.reply("textDocument/definition", helper_definition(&s));
    let req = s.request();
    let mut batches = Vec::new();
    let answer = s
        .navigation
        .references(&req, &AtomicBool::new(false), &mut |batch| {
            batches.push(batch)
        })
        .unwrap();
    assert_eq!(answer.provider, LspNavigation::NAME);
    assert_eq!(answer.value.word.as_deref(), Some("helper"));
    assert!(!answer.value.cancelled);
    let sent = s
        .fake
        .records()
        .into_iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "textDocument/references" => {
                Some(params)
            }
            _ => None,
        });
    assert_eq!(sent.unwrap()["context"]["includeDeclaration"], json!(true));

    // One file a batch, the request's first.
    assert!(batches.iter().all(|batch| batch.len() == 1), "{batches:?}");
    let files: Vec<FileReferences> = batches.into_iter().flatten().collect();
    let rels: Vec<_> = files
        .iter()
        .map(|file| file.rel.to_str().unwrap())
        .collect();
    assert_eq!(rels, ["src/main.rs", "src/far.rs", "src/other.rs"]);
    let refs: Vec<_> = (files.iter().flat_map(|file| &file.refs))
        .map(|reference| {
            (
                reference.location.range.clone(),
                reference.line_number,
                reference.preview.as_str(),
                reference.preview_ranges.clone(),
                reference.verified,
                reference.is_definition,
            )
        })
        .collect();
    let far_text = FAR.replace("\r\n", "\n");
    let far_at = far_text.find(FAR_ROW).unwrap();
    let call = bytes_of(MAIN, "helper();");
    assert_eq!(
        refs,
        [
            (3..9, 1, "fn helper() {}", one(3..9), true, true),
            (
                call.start..call.start + 6,
                4,
                "    helper();",
                one(4..10),
                true,
                false
            ),
            (
                far_at + far_name.start..far_at + far_name.end,
                2,
                FAR_ROW,
                one(far_name.clone()),
                true,
                false
            ),
            (7..12, 1, "pub fn other() {}", one(7..12), true, false),
        ]
    );

    // Workspace symbols.
    s.reply(
        "workspace/symbol",
        json!([
            {
                "name": "helper",
                "kind": 12,
                "location": location(&main, range(0, 3, 9)),
                "containerName": "main",
            },
            {
                "name": "Helper",
                "kind": 23,
                "location": location(&far, range(1, far_name.start, far_name.end)),
            },
        ]),
    );
    assert_eq!(s.lsp.readiness(None), Readiness::Ready);
    let answer = s
        .navigation
        .workspace_symbols("hlp", 10, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(answer.provider, LspNavigation::NAME);
    let sent = s
        .fake
        .records()
        .into_iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "workspace/symbol" => Some(params),
            _ => None,
        });
    assert_eq!(sent.unwrap()["query"], json!("hlp"));
    let found: Vec<_> = (answer.value.iter())
        .map(|symbol| {
            (
                symbol.name.as_str(),
                symbol.kind,
                symbol.container.as_deref(),
                symbol.location.rel.to_str().unwrap(),
                symbol.location.range.clone(),
                symbol.location.snapshot,
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "helper",
                OutlineKind::Function,
                Some("main"),
                "src/main.rs",
                3..9,
                Some((s.main.id(), s.main.version())),
            ),
            (
                "Helper",
                OutlineKind::Struct,
                None,
                "src/far.rs",
                far_at + far_name.start..far_at + far_name.end,
                None,
            ),
        ]
    );
    // Highlighted as search's own symbols are.
    for symbol in &answer.value {
        assert_eq!(symbol.indices, search_indices(&symbol.name, "hlp"));
    }
}

/// The chars of `name` that search's index matches `query` with.
fn search_indices(name: &str, query: &str) -> Vec<usize> {
    let mut index = SymbolIndex::new("/nowhere");
    index.extend(vec![FileSymbols {
        rel: "a.rs".into(),
        language: "rust",
        len: 0,
        modified: None,
        symbols: vec![Symbol {
            name: name.into(),
            kind: OutlineKind::Function,
            name_range: 0..0,
            range: 0..0,
            row: 0,
            container: None,
        }],
        truncated: false,
    }]);
    let [found] = &index.fuzzy(query, 1)[..] else {
        panic!("{name} doesn't match {query}");
    };
    assert!(!found.indices.is_empty());
    found.indices.clone()
}

/// Runs `ask` on a thread; panics if it takes longer than [`WAIT`] of real
/// time (an answer held up by a request that never replies).
fn promptly<T: Send + 'static>(ask: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || sender.send(ask()));
    receiver.recv_timeout(WAIT).expect("an answer in time")
}

#[test]
fn references_and_symbols_without_what_they_need() {
    let s = Nav::ready(ReadinessMode::ServerStatus);
    let gone = Url::from_file_path(s.root.join("src/gone.rs")).unwrap();

    // A final empty answer stands, whatever its definitions say.
    s.reply("textDocument/references", json!([]));
    s.reply(
        "textDocument/definition",
        json!([location(&gone, range(0, 0, 4))]),
    );
    let answer = s
        .navigation
        .references(&s.request(), &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert_eq!(answer.provider, LspNavigation::NAME);
    assert_eq!(s.searched.load(Ordering::SeqCst), 0);

    // Failing or empty references don't wait for definitions that never
    // come.
    s.fake
        .set_reply("textDocument/definition", Scripted::now(Reply::Never));
    for (references, unavailable) in [
        (
            Reply::Error {
                code: -32801,
                message: "content modified".into(),
            },
            true,
        ),
        (Reply::Result(json!([])), false),
    ] {
        s.fake
            .set_reply("textDocument/references", Scripted::now(references));
        let (lsp, req) = (s.lsp.clone(), s.request());
        let answer = promptly(move || lsp.references(&req, &AtomicBool::new(false), &mut |_| {}));
        assert_eq!(
            matches!(answer, Err(NavError::Unavailable(_))),
            unavailable,
            "{answer:?}"
        );
    }

    // Symbols none of which can be shown aren't a final "none".
    s.reply(
        "workspace/symbol",
        json!([{ "name": "helper", "kind": 12, "location": location(&gone, range(0, 0, 6)) }]),
    );
    let answer = s.lsp.workspace_symbols("hlp", 10, &AtomicBool::new(false));
    assert!(
        matches!(answer, Err(NavError::Unavailable(_))),
        "{answer:?}"
    );

    // Ranked by the matcher before the limit: the exact name comes first,
    // however late the server gave it.
    let main = s.uri("src/main.rs");
    s.reply(
        "workspace/symbol",
        json!([
            { "name": "has_long_path", "kind": 12, "location": location(&main, range(0, 3, 9)) },
            { "name": "hlp", "kind": 12, "location": location(&main, range(0, 3, 9)) },
        ]),
    );
    let answer = s
        .lsp
        .workspace_symbols("hlp", 1, &AtomicBool::new(false))
        .unwrap();
    let names: Vec<_> = answer.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(names, ["hlp"]);
    assert_eq!(answer[0].indices, [0, 1, 2]);
}

#[test]
fn definitions_alongside_references_time_out_on_their_own() {
    let s = Nav::ready(ReadinessMode::ServerStatus);
    let main = s.uri("src/main.rs");
    // On the fake's clock: the definition at 10 s, the references at 20 s.
    let later = |secs, value| Scripted {
        reply: Reply::Result(value),
        delay: Duration::from_secs(secs),
    };
    s.fake
        .set_reply("textDocument/definition", later(10, helper_definition(&s)));
    s.fake.set_reply(
        "textDocument/references",
        later(20, json!([location(&main, range(0, 3, 9))])),
    );
    let (lsp, req) = (s.lsp.clone(), s.request());
    let (sender, answer) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut files = Vec::new();
        let summary = lsp.references(&req, &AtomicBool::new(false), &mut |batch| {
            files.extend(batch)
        });
        sender.send((summary, files))
    });
    let id_of = |records: &[Record], wanted: &str| {
        records.iter().find_map(|record| match record {
            Record::Request { id, method, .. } if method == wanted => Some(id.clone()),
            _ => None,
        })
    };
    let records = s.fake.wait_for("both requests", |records| {
        id_of(records, "textDocument/definition").is_some()
            && id_of(records, "textDocument/references").is_some()
    });
    let NumberOrString::Number(definition) = id_of(&records, "textDocument/definition").unwrap()
    else {
        panic!("not a number");
    };

    // The definition's deadline passes while the references are awaited:
    // it's cancelled then.
    let timeout = Duration::from_millis(LspSettings::default().timeouts.definition);
    s.clock.wait_for_sleep(timeout);
    s.clock.advance(timeout);
    s.fake.wait_for("the definition's cancel", |records| {
        records.iter().any(|record| match record {
            Record::Notification { method, params } => {
                method == "$/cancelRequest" && params["id"] == json!(definition)
            }
            _ => false,
        })
    });
    assert!(
        answer.try_recv().is_err(),
        "still waiting for the references"
    );

    // Its late reply marks nothing.
    s.fake_clock.wait_for_sleep(Duration::from_secs(20));
    s.fake_clock.advance(Duration::from_secs(20));
    let (summary, files) = answer.recv_timeout(WAIT).unwrap();
    summary.unwrap();
    let marked: Vec<_> = (files.iter().flat_map(|file| &file.refs))
        .map(|reference| (reference.location.range.clone(), reference.is_definition))
        .collect();
    assert_eq!(marked, [(3..9, false)]);
}
