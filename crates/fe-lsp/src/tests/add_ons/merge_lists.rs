//! Merged lists (§5, §7 Merging): hover, locations, code actions, lenses,
//! inlay hints, links and colours from both servers, in server order, with
//! one server's failure or timeout tolerated.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fe_buffer::Buffer;
use fe_nav::{FileReferences, NavRequest, NavigationProvider};
use futures::FutureExt;
use futures::future::BoxFuture;
use lsp_types::{
    CodeActionProviderCapability, CodeLensOptions, ColorProviderCapability, DocumentLinkOptions,
    HoverProviderCapability, OneOf, PositionEncodingKind, ServerCapabilities, Url,
};
use serde_json::{Value, json};

use super::{CSS, Fixture, ROOT, answer, spec};
use crate::fake::{
    Echo, FakeLaunchers, FakeServer, InitializeReply, Record, Reply, Script, Scripted,
    initialize_result,
};
use crate::{Hover, LspManager, LspNavigation, ManualClock, ServerId, ServerStatus};

const HOVER: &str = "textDocument/hover";
const DEFINITION: &str = "textDocument/definition";
const REFERENCES: &str = "textDocument/references";
const CODE_ACTION: &str = "textDocument/codeAction";
const CODE_LENS: &str = "textDocument/codeLens";
const INLAY_HINT: &str = "textDocument/inlayHint";
const DOCUMENT_LINK: &str = "textDocument/documentLink";
const DOCUMENT_COLOR: &str = "textDocument/documentColor";

/// Two rows: `a` at bytes 0..1, `b` at 5..6.
const TEXT: &str = "a {}\nb {}\n";

/// Every merged feature.
fn everything(capabilities: &mut ServerCapabilities) {
    capabilities.hover_provider = Some(HoverProviderCapability::Simple(true));
    capabilities.definition_provider = Some(OneOf::Left(true));
    capabilities.references_provider = Some(OneOf::Left(true));
    capabilities.code_action_provider = Some(CodeActionProviderCapability::Simple(true));
    capabilities.code_lens_provider = Some(CodeLensOptions {
        resolve_provider: Some(false),
    });
    capabilities.inlay_hint_provider = Some(OneOf::Left(true));
    capabilities.document_link_provider = Some(DocumentLinkOptions {
        resolve_provider: Some(false),
        work_done_progress_options: Default::default(),
    });
    capabilities.color_provider = Some(ColorProviderCapability::Simple(true));
}

/// Both servers providing everything, with the document open on both,
/// and the manager behind a mutex, as `LspNavigation` wants it.
struct Merged {
    f: Fixture,
    manager: Arc<Mutex<LspManager>>,
    lsp: LspNavigation,
    buffer: Buffer,
    uri: Url,
    css: ServerId,
    vars: ServerId,
}

impl Merged {
    fn new() -> Self {
        let mut f = Fixture::css_and_vars_with(everything, everything);
        let mut buffer = Buffer::new(TEXT);
        let uri = f.open_on_both(&mut buffer);
        let css = f.placed(&buffer, "css").unwrap();
        let vars = f.placed(&buffer, "vars").unwrap();
        // One with no specs takes the fixture's place.
        let (idle, _) = super::super::manager(f.fake("css").launcher(), None);
        let manager = Arc::new(Mutex::new(std::mem::replace(&mut f.manager, idle)));
        let lsp = LspNavigation::new(Arc::downgrade(&manager), ROOT);
        Self {
            f,
            manager,
            lsp,
            buffer,
            uri,
            css,
            vars,
        }
    }

    /// Fake `name` answers `method` with `reply`, at once.
    fn reply(&self, name: &str, method: &str, reply: Reply) {
        self.f.fake(name).set_reply(method, Scripted::now(reply));
    }

    /// A location in the document, on row `row`'s first character.
    fn at(&self, row: u32) -> Value {
        json!({ "uri": self.uri, "range": first_of(row) })
    }

    fn request(&self, offset: usize) -> NavRequest {
        NavRequest {
            root: PathBuf::from(ROOT),
            rel: PathBuf::from("a.css"),
            language: Some(CSS),
            text: self.buffer.text().clone(),
            offset,
            buffer: Some((self.buffer.id(), self.buffer.version())),
        }
    }

    /// The definitions' ranges.
    fn definitions(&self) -> Vec<std::ops::Range<usize>> {
        let found = (self.lsp)
            .definitions(&self.request(0), &AtomicBool::new(false))
            .unwrap();
        (found.items.into_iter())
            .map(|item| item.location.range)
            .collect()
    }

    /// The references' ranges.
    fn references(&self) -> Vec<std::ops::Range<usize>> {
        let mut files: Vec<FileReferences> = Vec::new();
        (self.lsp)
            .references(&self.request(0), &AtomicBool::new(false), &mut |found| {
                files.extend(found);
            })
            .unwrap();
        (files.iter().flat_map(|file| &file.refs))
            .map(|reference| reference.location.range.clone())
            .collect()
    }
}

/// The first character of row `row`.
fn first_of(row: u32) -> Value {
    json!({
        "start": { "line": row, "character": 0 },
        "end": { "line": row, "character": 1 },
    })
}

/// A colour of `red` over `range`.
fn color(range: Value, red: f32) -> Value {
    json!({ "range": range, "color": { "red": red, "green": 0, "blue": 0, "alpha": 1 } })
}

#[test]
fn merged_lists_keep_server_order_and_precedence() {
    let m = Merged::new();
    let manager = m.manager.lock().unwrap();
    let (id, version) = (m.buffer.id(), m.buffer.version());

    // Hover contents join in order, a repeat dropped; the first range.
    m.reply(
        "css",
        HOVER,
        Reply::Result(json!({ "contents": "from css" })),
    );
    m.reply(
        "vars",
        HOVER,
        Reply::Result(json!({ "contents": "from vars", "range": first_of(0) })),
    );
    let hover = answer(manager.hover(id, version, 0)).unwrap();
    assert_eq!(
        hover,
        Some(Hover::new("from css\n\n---\n\nfrom vars", Some(0..1)))
    );
    m.reply(
        "vars",
        HOVER,
        Reply::Result(json!({ "contents": "from css" })),
    );
    let hover = answer(manager.hover(id, version, 0)).unwrap();
    assert_eq!(
        hover,
        Some(Hover::new("from css", None)),
        "a repeat dropped"
    );

    // Two links or two colours at one range keep the earlier server's.
    let link = |range: Value, target: &str| json!({ "range": range, "target": target });
    m.reply(
        "css",
        DOCUMENT_LINK,
        Reply::Result(json!([link(first_of(1), "https://css/1")])),
    );
    m.reply(
        "vars",
        DOCUMENT_LINK,
        Reply::Result(json!([
            link(first_of(0), "https://vars/0"),
            link(first_of(1), "https://vars/1"),
        ])),
    );
    let links = answer(manager.document_links(id, version))
        .unwrap()
        .unwrap();
    assert_eq!(
        (links.iter())
            .map(|link| (
                link.range.clone(),
                link.target.as_deref().unwrap(),
                link.owner().unwrap().server
            ))
            .collect::<Vec<_>>(),
        [
            (0..1, "https://vars/0", m.vars),
            (5..6, "https://css/1", m.css)
        ]
    );
    m.reply(
        "css",
        DOCUMENT_COLOR,
        Reply::Result(json!([color(first_of(1), 0.5)])),
    );
    m.reply(
        "vars",
        DOCUMENT_COLOR,
        Reply::Result(json!([color(first_of(1), 1.0), color(first_of(0), 1.0)])),
    );
    let colors = answer(manager.document_colors(id, version))
        .unwrap()
        .unwrap();
    assert_eq!(
        (colors.iter())
            .map(|info| (
                info.range.clone(),
                info.color[0],
                info.owner().unwrap().server
            ))
            .collect::<Vec<_>>(),
        [(0..1, 1.0, m.vars), (5..6, 0.5, m.css)]
    );

    // Two hints at one position both show, in server order; lenses are
    // ordered by position, then server.
    let hint = |label: &str| json!({ "position": { "line": 0, "character": 1 }, "label": label });
    m.reply("css", INLAY_HINT, Reply::Result(json!([hint("css")])));
    m.reply("vars", INLAY_HINT, Reply::Result(json!([hint("vars")])));
    let hints = answer(manager.inlay_hints(id, version, 0..TEXT.len()))
        .unwrap()
        .unwrap();
    assert_eq!(
        (hints.iter())
            .map(|hint| (hint.offset, hint.label.as_str()))
            .collect::<Vec<_>>(),
        [(1, "css"), (1, "vars")]
    );
    let lens = |row: u32| json!({ "range": first_of(row) });
    m.reply("css", CODE_LENS, Reply::Result(json!([lens(1)])));
    m.reply("vars", CODE_LENS, Reply::Result(json!([lens(1), lens(0)])));
    let lenses = answer(manager.code_lenses(id, version)).unwrap().unwrap();
    assert_eq!(
        (lenses.iter())
            .map(|lens| lens.range.clone())
            .collect::<Vec<_>>(),
        [0..1, 5..6, 5..6]
    );
    drop(manager);

    // Definitions and references are deduped by location, the first
    // server's order kept.
    let both = |m: &Merged, method: &str| {
        m.reply("css", method, Reply::Result(json!([m.at(1), m.at(0)])));
        m.reply("vars", method, Reply::Result(json!([m.at(0), m.at(1)])));
    };
    both(&m, DEFINITION);
    assert_eq!(m.definitions(), [5..6, 0..1]);
    both(&m, REFERENCES);
    assert_eq!(m.references(), [0..1, 5..6]);
}

/// What asks the manager, giving whether the answer is as expected.
type Ask = Box<dyn FnOnce(&LspManager) -> BoxFuture<'static, bool> + Send>;

/// How the failing server fails.
#[derive(Clone, Copy, Debug)]
enum Failing {
    /// The primary answers each request with an error.
    PrimaryErrs,
    /// The add-on never answers, and each request times out.
    AddOnSilent,
}

#[test]
fn one_server_failing_still_gives_the_others_results() {
    for failing in [Failing::PrimaryErrs, Failing::AddOnSilent] {
        let m = Merged::new();
        let (good, bad, kept) = match failing {
            Failing::PrimaryErrs => ("vars", "css", m.vars),
            Failing::AddOnSilent => ("css", "vars", m.css),
        };
        let fail = match failing {
            Failing::PrimaryErrs => Reply::Error {
                code: -32603,
                message: "broken".into(),
            },
            Failing::AddOnSilent => Reply::Never,
        };
        let replies: [(&str, Value); 8] = [
            (HOVER, json!({ "contents": good })),
            (DEFINITION, json!([m.at(1)])),
            (REFERENCES, json!([m.at(1)])),
            (CODE_ACTION, json!([{ "title": good }])),
            (CODE_LENS, json!([{ "range": first_of(1) }])),
            (
                INLAY_HINT,
                json!([{ "position": { "line": 1, "character": 1 }, "label": good }]),
            ),
            (
                DOCUMENT_LINK,
                json!([{ "range": first_of(1), "target": "https://x/" }]),
            ),
            (DOCUMENT_COLOR, json!([color(first_of(1), 1.0)])),
        ];
        for (method, reply) in replies {
            m.reply(good, method, Reply::Result(reply));
            m.reply(bad, method, fail.clone());
        }
        let why = format!("{failing:?}");
        // Asks with `ask` on a thread, and once both servers have the
        // request (and the good one's answer has been read), lets every
        // timeout pass.
        let settled = |method: &'static str, ask: Box<dyn FnOnce() -> bool + Send>| {
            let asked = std::thread::spawn(ask);
            for name in [good, bad] {
                m.f.fake(name).wait_for("the request", |records| {
                    (records.iter()).any(|record| record.method() == method)
                });
            }
            m.f.fake(good).wait_for("the answer", |records| {
                (records.iter()).any(
                    |record| matches!(record, Record::Replied { method: m, .. } if m == method),
                )
            });
            for server in [m.css, m.vars] {
                let echo = m.manager.lock().unwrap().request::<Echo>(server, json!({}));
                answer(echo).unwrap();
            }
            m.f.clock.advance(Duration::from_secs(60));
            assert!(asked.join().unwrap(), "{method}: {why}");
        };
        let (id, version) = (m.buffer.id(), m.buffer.version());
        // Asked under the manager's lock, answered outside it.
        let ask = |make: Ask| {
            let manager = m.manager.clone();
            Box::new(move || {
                let asked = make(&manager.lock().unwrap());
                answer(asked)
            }) as Box<dyn FnOnce() -> bool + Send>
        };
        settled(
            HOVER,
            ask(Box::new(move |manager| {
                let asked = manager.hover(id, version, 0);
                let text = good.to_owned();
                (async move { asked.await.unwrap().is_some_and(|hover| hover.text == text) })
                    .boxed()
            })),
        );
        settled(
            CODE_ACTION,
            ask(Box::new(move |manager| {
                let asked = manager.code_actions(id, version, 0..1, Vec::new());
                (async move {
                    let found = asked.await.unwrap();
                    found.len() == 1 && found[0].title == good && found[0].server() == kept
                })
                .boxed()
            })),
        );
        settled(
            CODE_LENS,
            ask(Box::new(move |manager| {
                let asked = manager.code_lenses(id, version);
                (async move {
                    asked
                        .await
                        .unwrap()
                        .is_some_and(|found| found.len() == 1 && found[0].range == (5..6))
                })
                .boxed()
            })),
        );
        settled(
            INLAY_HINT,
            ask(Box::new(move |manager| {
                let asked = manager.inlay_hints(id, version, 0..TEXT.len());
                (async move {
                    asked.await.unwrap().is_some_and(|found| {
                        found.len() == 1 && found[0].label == good && found[0].offset == 6
                    })
                })
                .boxed()
            })),
        );
        settled(
            DOCUMENT_LINK,
            ask(Box::new(move |manager| {
                let asked = manager.document_links(id, version);
                (async move {
                    asked.await.unwrap().is_some_and(|found| {
                        found.len() == 1 && found[0].owner().unwrap().server == kept
                    })
                })
                .boxed()
            })),
        );
        settled(
            DOCUMENT_COLOR,
            ask(Box::new(move |manager| {
                let asked = manager.document_colors(id, version);
                (async move {
                    asked.await.unwrap().is_some_and(|found| {
                        found.len() == 1 && found[0].owner().unwrap().server == kept
                    })
                })
                .boxed()
            })),
        );
        // Navigation blocks its caller, and locks the manager only to ask.
        let lsp = Arc::new(LspNavigation::new(Arc::downgrade(&m.manager), ROOT));
        for method in [DEFINITION, REFERENCES] {
            let (lsp, request) = (lsp.clone(), m.request(0));
            settled(
                method,
                Box::new(move || {
                    let cancel = AtomicBool::new(false);
                    let ranges: Vec<_> = match method {
                        DEFINITION => (lsp.definitions(&request, &cancel).unwrap().items)
                            .into_iter()
                            .map(|item| item.location.range)
                            .collect(),
                        _ => {
                            let mut ranges = Vec::new();
                            lsp.references(&request, &cancel, &mut |files| {
                                for file in files {
                                    ranges.extend(file.refs.into_iter().map(|r| r.location.range));
                                }
                            })
                            .unwrap();
                            ranges
                        }
                    };
                    ranges.len() == 1 && ranges[0] == (5..6)
                }),
            );
        }
    }
}

#[test]
fn navigation_asks_the_ready_servers_without_waiting_for_one_starting() {
    // The primary never finishes starting; the add-on is ready.
    let clock = Arc::new(ManualClock::new());
    let starting = Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    };
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    everything(&mut init.capabilities);
    let ready = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let fakes = vec![
        ("css", FakeServer::with_clock(starting, clock.clone())),
        ("vars", FakeServer::with_clock(ready, clock.clone())),
    ];
    let launchers = FakeLaunchers::new(fakes.iter().map(|(name, fake)| (*name, fake.launcher())));
    let mut f = Fixture::with_clock(fakes, Arc::new(launchers), clock);
    f.manager.register_spec(spec("css", &[CSS], false));
    f.manager.register_spec(spec("vars", &[CSS], true));
    let mut buffer = Buffer::new(TEXT);
    let uri = f.open(&mut buffer, "a.css", CSS);
    f.wait_synced("vars", &uri, &buffer);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while f.manager.status_at("vars", Path::new(ROOT)) != Some(ServerStatus::Ready) {
        assert!(
            std::time::Instant::now() < deadline,
            "the add-on never became ready"
        );
        f.clock.advance(crate::readiness::SETTLE);
        std::thread::sleep(Duration::from_millis(5));
    }
    let at = json!([{ "uri": uri, "range": first_of(1) }]);
    f.fake("vars")
        .set_reply(DEFINITION, Scripted::now(Reply::Result(at)));

    let (idle, _) = super::super::manager(f.fake("vars").launcher(), None);
    let manager = Arc::new(Mutex::new(std::mem::replace(&mut f.manager, idle)));
    let lsp = Arc::new(LspNavigation::new(Arc::downgrade(&manager), ROOT));
    assert_eq!(lsp.readiness(Some(CSS)), fe_nav::Readiness::Ready);
    let request = NavRequest {
        root: PathBuf::from(ROOT),
        rel: PathBuf::from("a.css"),
        language: Some(CSS),
        text: buffer.text().clone(),
        offset: 0,
        buffer: Some((buffer.id(), buffer.version())),
    };
    // With the clock still, waiting for the primary would never end.
    let (sent, found) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let found = lsp.definitions(&request, &AtomicBool::new(false));
        let _ = sent.send(found.map(|found| {
            (found.items.into_iter())
                .map(|item| item.location.range)
                .collect::<Vec<_>>()
        }));
    });
    let found = found
        .recv_timeout(Duration::from_secs(10))
        .expect("an answer without waiting for the primary");
    assert_eq!(found.unwrap(), vec![5..6]);
}
