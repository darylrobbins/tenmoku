//! Document symbols: both reply shapes decoded in bytes of the version in a
//! UTF-8 and a UTF-16 session, nesting, kinds, the cap, and a server without
//! the provider, a `null` reply and a timeout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use fe_nav::OutlineKind;
use futures::FutureExt;
use lsp_types::request::{DocumentSymbolRequest, Request};
use lsp_types::{OneOf, Position, PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::{DocumentSymbol, MAX_SYMBOLS, outline_kind};
use crate::event::LspError;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{Clock, LspEvent, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = crate::fake::FAKE_ROOT;

/// Waits for `reply`; panics after 10 s of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = SystemClock.sleep(Duration::from_secs(10)).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in 10 s"),
        }
    })
}

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
    uri: Url,
    utf16: bool,
}

impl Started {
    fn new(encoding: PositionEncodingKind, provider: bool, text: &str) -> Self {
        let utf16 = encoding == PositionEncodingKind::UTF16;
        let mut init = initialize_result(Some(encoding));
        init.capabilities = ServerCapabilities {
            document_symbol_provider: provider.then_some(OneOf::Left(true)),
            ..init.capabilities
        };
        let fake = FakeServer::with_clock(
            Script {
                initialize: InitializeReply::Result(Box::new(init)),
                ..Script::default()
            },
            Arc::new(ManualClock::new()),
        );
        let clock = Arc::new(ManualClock::new());
        let (sender, events) = std::sync::mpsc::channel::<LspEvent>();
        let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), move |event| {
            let _ = sender.send(event);
        });
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
            clock,
            manager,
            buffer,
            uri,
            utf16,
        }
    }

    /// The LSP position of byte `offset` of `text`, counted as the protocol
    /// says, not with fe-lsp's converter.
    fn position(&self, text: &str, offset: usize) -> Position {
        let before = &text[..offset];
        let line = before.matches('\n').count();
        let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
        let column = if self.utf16 {
            column.encode_utf16().count()
        } else {
            column.len()
        };
        Position::new(line as u32, column as u32)
    }

    fn range(&self, text: &str, range: std::ops::Range<usize>) -> Value {
        json!({
            "start": self.position(text, range.start),
            "end": self.position(text, range.end),
        })
    }

    /// A `DocumentSymbol` over `range`, selecting `select`.
    fn symbol(
        &self,
        text: &str,
        name: &str,
        kind: u32,
        range: std::ops::Range<usize>,
        select: std::ops::Range<usize>,
        children: Vec<Value>,
    ) -> Value {
        json!({
            "name": name,
            "kind": kind,
            "range": self.range(text, range),
            "selectionRange": self.range(text, select),
            "children": children,
        })
    }

    /// A `SymbolInformation` in `uri`.
    fn info(
        &self,
        text: &str,
        uri: &Url,
        name: &str,
        kind: u32,
        range: std::ops::Range<usize>,
    ) -> Value {
        json!({
            "name": name,
            "kind": kind,
            "location": { "uri": uri, "range": self.range(text, range) },
        })
    }

    fn reply(&self, reply: Value) {
        self.fake.set_reply(
            DocumentSymbolRequest::METHOD,
            Scripted::now(Reply::Result(reply)),
        );
    }

    fn symbols(&self) -> Result<Option<Vec<DocumentSymbol>>, LspError> {
        answer(
            self.manager
                .document_symbols(self.buffer.id(), self.buffer.version()),
        )
    }

    fn requests(&self) -> Vec<Value> {
        symbol_requests(&self.fake.records())
    }
}

fn symbol_requests(records: &[Record]) -> Vec<Value> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == DocumentSymbolRequest::METHOD => {
                Some(params.clone())
            }
            _ => None,
        })
        .collect()
}

/// (name, depth, parent, range) of each symbol.
fn shape(symbols: &[DocumentSymbol]) -> Vec<(&str, u32, Option<u32>, std::ops::Range<usize>)> {
    (symbols.iter())
        .map(|s| (s.name.as_str(), s.depth, s.parent, s.range.clone()))
        .collect()
}

#[test]
fn hierarchical_symbols_are_pre_order_with_depth_and_parent() {
    let text = "struct A {\n    x: u8,\n    y: u8,\n}\nfn b() {}\n";
    let a = 0..text.find("}\n").unwrap() + 1;
    let x = text.find("x: u8,").unwrap();
    let y = text.find("y: u8,").unwrap();
    let b = text.find("fn b").unwrap();
    let s = Started::new(PositionEncodingKind::UTF8, true, text);
    // Out of order, with the children out of order too.
    let mut outer = s.symbol(
        text,
        "A",
        23,
        a.clone(),
        7..8,
        vec![
            s.symbol(text, "y", 8, y..y + 6, y..y + 1, vec![]),
            s.symbol(text, "x", 8, x..x + 6, x..x + 1, vec![]),
        ],
    );
    outer["detail"] = json!("a struct");
    let last = s.symbol(text, "b", 12, b..text.len() - 1, b + 3..b + 4, vec![]);
    s.reply(json!([last, outer]));
    let found = s.symbols().unwrap().unwrap();
    assert_eq!(
        shape(&found),
        vec![
            ("A", 0, None, a.clone()),
            ("x", 1, Some(0), x..x + 6),
            ("y", 1, Some(0), y..y + 6),
            ("b", 0, None, b..text.len() - 1),
        ]
    );
    assert_eq!(found[0].detail, "a struct");
    assert_eq!(found[0].selection_range, 7..8);
    assert_eq!(found[0].kind, OutlineKind::Struct);
    assert!(!found[0].deprecated);
    let params = s.requests().pop().unwrap();
    assert_eq!(params["textDocument"]["uri"], json!(s.uri));
}

#[test]
fn utf16_ranges_decode_to_bytes() {
    // An emoji before the symbols: four bytes, two UTF-16 units.
    let text = "let 🦀 = 1; fn alpha() { fn beta() {} }\n";
    let alpha = text.find("fn alpha").unwrap();
    let beta = text.find("fn beta").unwrap();
    let end = text.rfind('}').unwrap() + 1;
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let s = Started::new(encoding, true, text);
        s.reply(json!([s.symbol(
            text,
            "alpha",
            12,
            alpha..end,
            alpha + 3..alpha + 8,
            vec![s.symbol(text, "beta", 12, beta..end - 2, beta + 3..beta + 7, vec![])],
        )]));
        let found = s.symbols().unwrap().unwrap();
        assert_eq!(
            shape(&found),
            vec![
                ("alpha", 0, None, alpha..end),
                ("beta", 1, Some(0), beta..end - 2),
            ]
        );
        assert_eq!(found[1].selection_range, beta + 3..beta + 7);
    }
}

#[test]
fn selection_is_clipped_into_range_and_inverted_ranges_dropped() {
    let text = "fn a() {}\nfn b() {}\n";
    let b = text.find("fn b").unwrap();
    let s = Started::new(PositionEncodingKind::UTF8, true, text);
    let mut bad = s.symbol(text, "bad", 12, 0..9, 0..1, vec![]);
    bad["range"] = json!({ "start": s.position(text, 9), "end": s.position(text, 0) });
    s.reply(json!([
        bad,
        // The selection reaches past the range.
        s.symbol(text, "a", 12, 0..9, 3..text.len(), vec![]),
        s.symbol(text, "b", 12, b..b + 9, b..b + 2, vec![]),
    ]));
    let found = s.symbols().unwrap().unwrap();
    assert_eq!(
        shape(&found),
        vec![("a", 0, None, 0..9), ("b", 0, None, b..b + 9)]
    );
    assert_eq!(found[0].selection_range, 3..9);
}

#[test]
fn flat_symbol_information_nests_by_containment() {
    let text = "mod m {\n    fn f() {}\n    fn g() {}\n}\nfn h() {}\n";
    let f = text.find("fn f").unwrap();
    let g = text.find("fn g").unwrap();
    let h = text.find("fn h").unwrap();
    let m_end = text.find("}\nfn h").unwrap() + 1;
    let s = Started::new(PositionEncodingKind::UTF16, true, text);
    s.reply(json!([
        s.info(text, &s.uri, "h", 12, h..h + 9),
        s.info(text, &s.uri, "g", 6, g..g + 9),
        s.info(text, &s.uri, "m", 2, 0..m_end),
        s.info(text, &s.uri, "f", 12, f..f + 9),
    ]));
    let found = s.symbols().unwrap().unwrap();
    assert_eq!(
        shape(&found),
        vec![
            ("m", 0, None, 0..m_end),
            ("f", 1, Some(0), f..f + 9),
            ("g", 1, Some(0), g..g + 9),
            ("h", 0, None, h..h + 9),
        ]
    );
    assert_eq!(found[0].kind, OutlineKind::Module);
    assert_eq!(found[2].kind, OutlineKind::Method);
    assert_eq!(found[1].selection_range, found[1].range);
}

#[test]
fn symbols_of_other_documents_are_dropped() {
    let text = "fn a() {}\n";
    let s = Started::new(PositionEncodingKind::UTF8, true, text);
    let other = Url::from_file_path(PathBuf::from(ROOT).join("b.rs")).unwrap();
    s.reply(json!([
        s.info(text, &other, "elsewhere", 12, 0..9),
        s.info(text, &s.uri, "a", 12, 0..9),
    ]));
    let found = s.symbols().unwrap().unwrap();
    assert_eq!(shape(&found), vec![("a", 0, None, 0..9)]);
}

#[test]
fn no_provider_sends_nothing() {
    let s = Started::new(PositionEncodingKind::UTF16, false, "fn a() {}\n");
    assert_eq!(s.symbols(), Ok(None));
    assert!(s.requests().is_empty());
}

#[test]
fn null_reply_and_timeout_are_empty() {
    let s = Started::new(PositionEncodingKind::UTF16, true, "fn a() {}\n");
    let method = DocumentSymbolRequest::METHOD;

    s.reply(Value::Null);
    assert_eq!(s.symbols(), Ok(Some(Vec::new())));
    s.reply(json!([]));
    assert_eq!(s.symbols(), Ok(Some(Vec::new())));

    // Past `lsp.timeouts.hover`'s 3 s.
    s.fake.set_reply(
        method,
        Scripted {
            reply: Reply::Result(json!([])),
            delay: Duration::from_secs(60),
        },
    );
    let count = s.requests().len();
    let mut late = s
        .manager
        .document_symbols(s.buffer.id(), s.buffer.version())
        .boxed();
    s.fake.wait_for("the late request", |records| {
        symbol_requests(records).len() > count
    });
    s.clock.wait_for_sleep(Duration::from_secs(3));
    s.clock.advance(Duration::from_millis(2999));
    std::thread::sleep(Duration::from_millis(50));
    assert!(late.as_mut().now_or_never().is_none());
    s.clock.advance(Duration::from_millis(1));
    assert_eq!(answer(late), Ok(Some(Vec::new())));
}

#[test]
fn stale_version_is_unavailable() {
    let mut s = Started::new(PositionEncodingKind::UTF8, true, "fn a() {}\n");
    s.reply(json!([]));
    let old = s.buffer.version();
    s.buffer.edit([(0..0, "// x\n")], Origin::User).unwrap();
    s.manager.changed(s.buffer.id());
    let edited = s.buffer.text().to_string();
    s.fake
        .wait_for_document(&s.uri, "the edit", |r| r.is_some_and(|r| r.text == edited));
    let found = answer(s.manager.document_symbols(s.buffer.id(), old));
    assert!(matches!(found, Err(LspError::Unavailable(_))), "{found:?}");
    assert!(s.requests().is_empty());
}

#[test]
fn kinds_map_to_outline_kinds() {
    use lsp_types::SymbolKind as K;
    for (kind, expected) in [
        (K::FILE, OutlineKind::Module),
        (K::MODULE, OutlineKind::Module),
        (K::NAMESPACE, OutlineKind::Module),
        (K::PACKAGE, OutlineKind::Module),
        (K::CLASS, OutlineKind::Class),
        (K::METHOD, OutlineKind::Method),
        (K::CONSTRUCTOR, OutlineKind::Method),
        (K::PROPERTY, OutlineKind::Field),
        (K::FIELD, OutlineKind::Field),
        (K::EVENT, OutlineKind::Field),
        (K::ENUM, OutlineKind::Enum),
        (K::INTERFACE, OutlineKind::Interface),
        (K::FUNCTION, OutlineKind::Function),
        (K::OPERATOR, OutlineKind::Function),
        (K::CONSTANT, OutlineKind::Constant),
        (K::ENUM_MEMBER, OutlineKind::Variant),
        (K::STRUCT, OutlineKind::Struct),
        (K::TYPE_PARAMETER, OutlineKind::Type),
        (K::VARIABLE, OutlineKind::Variable),
        (K::STRING, OutlineKind::Variable),
        (K::KEY, OutlineKind::Variable),
    ] {
        assert_eq!(outline_kind(kind), expected, "{kind:?}");
    }
}

#[test]
fn deprecated_tag_is_kept() {
    let text = "fn a() {}\nfn b() {}\nfn c() {}\n";
    let s = Started::new(PositionEncodingKind::UTF8, true, text);
    let mut tagged = s.symbol(text, "a", 12, 0..9, 3..4, vec![]);
    tagged["tags"] = json!([1]);
    let mut flagged = s.symbol(text, "b", 12, 10..19, 13..14, vec![]);
    flagged["deprecated"] = json!(true);
    s.reply(json!([
        tagged,
        flagged,
        s.symbol(text, "c", 12, 20..29, 23..24, vec![])
    ]));
    let found = s.symbols().unwrap().unwrap();
    let marks: Vec<_> = found.iter().map(|s| s.deprecated).collect();
    assert_eq!(marks, [true, true, false]);

    // The flat shape too.
    let mut info = s.info(text, &s.uri, "a", 12, 0..9);
    info["tags"] = json!([1]);
    s.reply(json!([info]));
    assert!(s.symbols().unwrap().unwrap()[0].deprecated);
}

#[test]
fn more_than_the_cap_are_truncated() {
    let n = MAX_SYMBOLS + 5;
    let text = "a\n".repeat(n);
    let s = Started::new(PositionEncodingKind::UTF8, true, &text);
    // One symbol per line, so positions are line numbers.
    let one = |at: usize| {
        json!({
            "name": "a",
            "kind": 13,
            "range": { "start": { "line": at, "character": 0 }, "end": { "line": at, "character": 1 } },
            "selectionRange": { "start": { "line": at, "character": 0 }, "end": { "line": at, "character": 1 } },
        })
    };
    // Flat top-level, then one parent with children: the cap holds for both.
    s.reply(Value::Array((0..n).map(one).collect()));
    let found = s.symbols().unwrap().unwrap();
    assert_eq!(found.len(), MAX_SYMBOLS);
    assert_eq!(
        found.last().unwrap().range,
        (MAX_SYMBOLS - 1) * 2..(MAX_SYMBOLS - 1) * 2 + 1
    );

    let parent = s.symbol(
        &text,
        "p",
        2,
        0..text.len(),
        0..1,
        (0..n).map(one).collect(),
    );
    s.reply(json!([parent]));
    let found = s.symbols().unwrap().unwrap();
    assert_eq!(found.len(), MAX_SYMBOLS);
    assert!(
        found[1..]
            .iter()
            .all(|s| s.parent == Some(0) && s.depth == 1)
    );
}
