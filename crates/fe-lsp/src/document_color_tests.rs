//! Document colours on the fake server: decoding colours and
//! presentations, a server without the provider, and a `null` reply and a
//! timeout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::{ColorProviderCapability, PositionEncodingKind};
use serde_json::{Value, json};

use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{
    ColorInformation, ColorPresentation, LspError, LspManager, ManualClock, ServerSpec, SystemClock,
};

const ROOT: &str = "/fake/root";
const WAIT: Duration = Duration::from_secs(10);
const COLORS: &str = "textDocument/documentColor";
const PRESENTATIONS: &str = "textDocument/colorPresentation";

/// Two rows, each an emoji (two UTF-16 units, four bytes) before a colour.
const TEXT: &str = "/* 🦀 */ a { color: #ff0000; }\n/* 🦀 */ b { color: rgb(0 0 255 / 50%); }\n";

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
}

impl Started {
    fn new(provider: Option<ColorProviderCapability>) -> Self {
        let mut result = initialize_result(Some(PositionEncodingKind::UTF16));
        result.capabilities.color_provider = provider;
        let fake = FakeServer::new(Script {
            initialize: InitializeReply::Result(Box::new(result)),
            ..Script::default()
        });
        let clock = Arc::new(ManualClock::new());
        let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), |_| {});
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        fake.wait_initialized();
        let mut buffer = Buffer::new(TEXT);
        let path = PathBuf::from(ROOT).join("a.css");
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
                "css",
            )
            .unwrap();
        fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        Self {
            fake,
            clock,
            manager,
            buffer,
        }
    }

    fn provided() -> Self {
        Self::new(Some(ColorProviderCapability::Simple(true)))
    }

    fn reply(&self, method: &str, reply: Value) {
        self.fake
            .set_reply(method, Scripted::now(Reply::Result(reply)));
    }

    fn ask(&self) -> Option<Vec<ColorInformation>> {
        answer(
            self.manager
                .document_colors(self.buffer.id(), self.buffer.version()),
        )
        .unwrap()
    }

    fn present(&self, info: ColorInformation) -> Result<Vec<ColorPresentation>, LspError> {
        answer(
            self.manager
                .color_presentations(self.buffer.id(), self.buffer.version(), info),
        )
    }

    fn sent(&self, method: &str) -> Vec<Value> {
        sent(&self.fake.records(), method)
    }
}

fn sent(records: &[Record], method: &str) -> Vec<Value> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params.clone()),
            _ => None,
        })
        .collect()
}

/// Waits for `reply`; panics after [`WAIT`] of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
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

fn color(red: f64, green: f64, blue: f64, alpha: f64) -> Value {
    json!({ "red": red, "green": green, "blue": blue, "alpha": alpha })
}

#[test]
fn colors_decode_to_bytes_and_clamp() {
    let s = Started::provided();
    // The later row first, its components out of range.
    s.reply(
        COLORS,
        json!([
            {
                "range": range_of("rgb(0 0 255 / 50%)"),
                "color": color(-0.5, 0.0, 1.5, 0.5),
            },
            { "range": range_of("#ff0000"), "color": color(1.0, 0.0, 0.0, 1.0) },
        ]),
    );
    let colors = s.ask().unwrap();
    assert_eq!(
        colors,
        [
            ColorInformation::new(bytes_of("#ff0000"), [1., 0., 0., 1.]),
            ColorInformation::new(bytes_of("rgb(0 0 255 / 50%)"), [0., 0., 1., 0.5]),
        ]
    );
    let sent = s.sent(COLORS);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["textDocument"]["uri"], "file:///fake/root/a.css");

    // Options in place of `true` provide them too.
    let options = lsp_types::ColorProviderOptions {};
    let s = Started::new(Some(ColorProviderCapability::ColorProvider(options)));
    s.reply(
        COLORS,
        json!([{ "range": range_of("#ff0000"), "color": color(1.0, 0.0, 0.0, 1.0) }]),
    );
    assert_eq!(s.ask().unwrap().len(), 1);
}

#[test]
fn no_provider_sends_nothing() {
    for provider in [None, Some(ColorProviderCapability::Simple(false))] {
        let s = Started::new(provider);
        s.reply(
            COLORS,
            json!([{ "range": range_of("#ff0000"), "color": color(1.0, 0.0, 0.0, 1.0) }]),
        );
        s.reply(PRESENTATIONS, json!([{ "label": "red" }]));
        assert_eq!(s.ask(), None);
        let info = ColorInformation::new(bytes_of("#ff0000"), [1., 0., 0., 1.]);
        assert!(matches!(
            s.present(info),
            Err(LspError::Unavailable(why)) if why == "colorPresentation"
        ));
        assert!(s.sent(COLORS).is_empty());
        assert!(s.sent(PRESENTATIONS).is_empty());
    }
}

#[test]
fn color_presentations_decode_edits() {
    let s = Started::provided();
    s.reply(
        PRESENTATIONS,
        json!([
            {
                "label": "rgb(0 0 255 / 50%)",
                "textEdit": { "range": range_of("#ff0000"), "newText": "rgb(0 0 255 / 50%)" },
                "additionalTextEdits": [
                    { "range": range_of("color"), "newText": "background" },
                ],
            },
            { "label": "#0000ff80" },
        ]),
    );
    let info = ColorInformation::new(bytes_of("#ff0000"), [0., 0., 1., 0.5]);
    let presentations = s.present(info).unwrap();
    assert_eq!(presentations.len(), 2);
    assert_eq!(presentations[0].label, "rgb(0 0 255 / 50%)");
    assert_eq!(
        presentations[0].text_edit,
        Some((bytes_of("#ff0000"), "rgb(0 0 255 / 50%)".to_owned()))
    );
    assert_eq!(
        presentations[0].additional_edits,
        [(bytes_of("color"), "background".to_owned())]
    );
    assert_eq!(presentations[1].label, "#0000ff80");
    assert_eq!(presentations[1].text_edit, None);
    assert!(presentations[1].additional_edits.is_empty());
    // The colour and its range went as the protocol has them.
    let sent = s.sent(PRESENTATIONS);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["textDocument"]["uri"], "file:///fake/root/a.css");
    assert_eq!(sent[0]["range"], range_of("#ff0000"));
    assert_eq!(sent[0]["color"], color(0.0, 0.0, 1.0, 0.5));
}

#[test]
fn null_reply_and_timeout_are_empty() {
    let s = Started::provided();
    s.reply(COLORS, Value::Null);
    assert_eq!(s.ask(), Some(Vec::new()));
    s.reply(PRESENTATIONS, Value::Null);
    let info = ColorInformation::new(bytes_of("#ff0000"), [1., 0., 0., 1.]);
    assert_eq!(s.present(info.clone()), Ok(Vec::new()));

    // Past `lsp.timeouts.hover`'s 3 s: none, and the request is
    // cancelled.
    let late = |method: &str, reply: Value| {
        s.fake.set_reply(
            method,
            Scripted {
                reply: Reply::Result(reply),
                delay: Duration::from_secs(60),
            },
        );
    };
    late(COLORS, json!([]));
    late(PRESENTATIONS, json!([]));
    let colors = s
        .manager
        .document_colors(s.buffer.id(), s.buffer.version())
        .boxed();
    let presentations = s
        .manager
        .color_presentations(s.buffer.id(), s.buffer.version(), info)
        .boxed();
    let records = s.fake.wait_for("the late requests", |records| {
        sent(records, COLORS).len() == 2 && sent(records, PRESENTATIONS).len() == 2
    });
    let last_id = |method: &str| {
        (records.iter())
            .filter_map(|record| match record {
                Record::Request { id, method: m, .. } if m == method => Some(json!(id)),
                _ => None,
            })
            .next_back()
            .unwrap()
    };
    let (colors_id, presentations_id) = (last_id(COLORS), last_id(PRESENTATIONS));
    s.clock.wait_for_sleep(Duration::from_secs(3));
    s.clock.advance(Duration::from_secs(3));
    assert_eq!(answer(colors), Ok(Some(Vec::new())));
    assert_eq!(answer(presentations), Ok(Vec::new()));
    s.fake.wait_for("$/cancelRequest", |records| {
        let cancelled = |id: &Value| {
            records.iter().any(|record| {
                matches!(record, Record::Notification { method, params }
                    if method == "$/cancelRequest" && params["id"] == *id)
            })
        };
        cancelled(&colors_id) && cancelled(&presentations_id)
    });
}
