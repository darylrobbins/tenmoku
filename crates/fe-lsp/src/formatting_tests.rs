//! `textDocument/formatting` and `textDocument/rangeFormatting`: the two
//! requests and their options, edits as bytes of the version formatted,
//! a server with no formatter, nothing to change, a slow server and
//! overlapping edits.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::FormatOptions;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{Clock, LspError, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = "/fake/root";
const FORMATTING: &str = "textDocument/formatting";
const RANGE: &str = "textDocument/rangeFormatting";
const OPTIONS: FormatOptions = FormatOptions {
    tab_size: 2,
    insert_spaces: false,
};

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

/// The requests of `method` the fake got.
fn requests(fake: &FakeServer, method: &str) -> Vec<Value> {
    (fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

struct Started {
    fake: FakeServer,
    manager: LspManager,
    clock: Arc<ManualClock>,
    buffer: Buffer,
    uri: Url,
}

/// A server on the fake in UTF-16, with the given formatting providers,
/// with a buffer of `text` open and synced.
fn started(formatting: bool, range: bool, text: &str) -> Started {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities = ServerCapabilities {
        document_formatting_provider: formatting.then_some(OneOf::Left(true)),
        document_range_formatting_provider: range.then_some(OneOf::Left(true)),
        ..init.capabilities
    };
    let fake = FakeServer::with_clock(
        Script {
            initialize: InitializeReply::Result(Box::new(init)),
            replies: HashMap::new(),
            ..Script::default()
        },
        Arc::new(ManualClock::new()),
    );
    let clock = Arc::new(ManualClock::new());
    let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), |_| {});
    let server = manager
        .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
        .unwrap();
    fake.wait_initialized();
    let mut buffer = Buffer::new(text);
    let path = PathBuf::from(ROOT).join("a.rs");
    let subscription = buffer.subscribe();
    manager
        .open_on(
            server,
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            &path,
            "rust",
        )
        .unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    Started {
        fake,
        manager,
        clock,
        buffer,
        uri,
    }
}

fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> Value {
    json!({
        "range": {
            "start": { "line": start.0, "character": start.1 },
            "end": { "line": end.0, "character": end.1 },
        },
        "newText": text,
    })
}

#[test]
fn document_and_range() {
    // An emoji earlier on the line: two UTF-16 units, four bytes.
    let text = "let 🦀 =  1;\nlet b  = 2;\n";
    let s = started(true, true, text);
    let (id, version) = (s.buffer.id(), s.buffer.version());
    let second = text.find("let b").unwrap();

    // The whole document: sorted, as bytes of the version. The crab is
    // 2 UTF-16 units but 4 bytes, so "=  1" is at units 8.. and bytes 10..
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([
            edit((1, 5), (1, 7), " "),
            edit((0, 8), (0, 10), " "),
        ]))),
    );
    let edits = answer(s.manager.format(id, version, None, OPTIONS)).unwrap();
    let eq = text.find('=').unwrap();
    assert_eq!(
        edits,
        Some(vec![
            (eq + 1..eq + 3, " ".to_owned()),
            (second + 5..second + 7, " ".to_owned()),
        ])
    );
    let sent = requests(&s.fake, FORMATTING).pop().unwrap();
    assert_eq!(sent["textDocument"]["uri"], json!(s.uri));
    assert_eq!(sent["options"]["tabSize"], json!(2));
    assert_eq!(sent["options"]["insertSpaces"], json!(false));
    assert!(sent["options"].get("trimTrailingWhitespace").is_none());
    assert!(requests(&s.fake, RANGE).is_empty());

    // A range: rangeFormatting, in the server's encoding.
    s.fake.set_reply(
        RANGE,
        Scripted::now(Reply::Result(json!([edit((1, 5), (1, 7), " ")]))),
    );
    let options = FormatOptions {
        tab_size: 4,
        insert_spaces: true,
    };
    let edits = answer(
        s.manager
            .format(id, version, Some(second..text.len()), options),
    )
    .unwrap();
    assert_eq!(edits, Some(vec![(second + 5..second + 7, " ".to_owned())]));
    let sent = requests(&s.fake, RANGE).pop().unwrap();
    assert_eq!(sent["range"]["start"], json!({ "line": 1, "character": 0 }));
    assert_eq!(sent["range"]["end"], json!({ "line": 2, "character": 0 }));
    assert_eq!(sent["options"]["tabSize"], json!(4));
    assert_eq!(sent["options"]["insertSpaces"], json!(true));
    assert_eq!(requests(&s.fake, FORMATTING).len(), 1);

    // A range that starts after the crab is counted in UTF-16.
    let crab = text.find('=').unwrap();
    let _ = answer(s.manager.format(id, version, Some(crab..crab + 1), OPTIONS)).unwrap();
    let sent = requests(&s.fake, RANGE).pop().unwrap();
    assert_eq!(sent["range"]["start"], json!({ "line": 0, "character": 7 }));
    assert_eq!(sent["range"]["end"], json!({ "line": 0, "character": 8 }));
}

#[test]
fn missing_and_slow() {
    // No provider: no request, and not an empty reply.
    let s = started(false, false, "let a = 1;\n");
    let (id, version) = (s.buffer.id(), s.buffer.version());
    assert_eq!(
        answer(s.manager.format(id, version, None, OPTIONS)),
        Ok(None)
    );
    assert_eq!(
        answer(s.manager.format(id, version, Some(0..3), OPTIONS)),
        Ok(None)
    );
    assert!(requests(&s.fake, FORMATTING).is_empty());
    assert!(requests(&s.fake, RANGE).is_empty());

    // Only the whole document: a range isn't asked for.
    let s = started(true, false, "let a = 1;\n");
    let (id, version) = (s.buffer.id(), s.buffer.version());
    assert_eq!(
        answer(s.manager.format(id, version, Some(0..3), OPTIONS)),
        Ok(None)
    );
    assert!(requests(&s.fake, RANGE).is_empty());

    // Nothing to change.
    s.fake
        .set_reply(FORMATTING, Scripted::now(Reply::Result(json!([]))));
    assert_eq!(
        answer(s.manager.format(id, version, None, OPTIONS)),
        Ok(Some(vec![]))
    );
    s.fake
        .set_reply(FORMATTING, Scripted::now(Reply::Result(Value::Null)));
    assert_eq!(
        answer(s.manager.format(id, version, None, OPTIONS)),
        Ok(Some(vec![]))
    );

    // Overlapping edits.
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([
            edit((0, 0), (0, 5), "x"),
            edit((0, 4), (0, 6), "y"),
        ]))),
    );
    assert!(matches!(
        answer(s.manager.format(id, version, None, OPTIONS)),
        Err(LspError::RefusedEdit(_))
    ));

    // No reply in 3 s: a timeout, not nothing to change.
    s.fake.set_reply(
        FORMATTING,
        Scripted {
            reply: Reply::Never,
            delay: Duration::ZERO,
        },
    );
    let count = requests(&s.fake, FORMATTING).len();
    let mut slow = s.manager.format(id, version, None, OPTIONS).boxed();
    s.fake.wait_for("the slow request", |records| {
        let asked = (records.iter())
            .filter(|r| matches!(r, Record::Request { method, .. } if method == FORMATTING));
        asked.count() > count
    });
    s.clock.wait_for_sleep(Duration::from_secs(3));
    s.clock.advance(Duration::from_millis(2999));
    std::thread::sleep(Duration::from_millis(50));
    assert!(slow.as_mut().now_or_never().is_none());
    s.clock.advance(Duration::from_millis(1));
    assert_eq!(answer(slow), Err(LspError::Timeout));
}
