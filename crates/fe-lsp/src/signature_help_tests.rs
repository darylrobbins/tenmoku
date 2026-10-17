//! `textDocument/signatureHelp`: labels and parameters as byte ranges,
//! clamped actives, and the request's context and the server's triggers,
//! on the fake.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{Request, SignatureHelpRequest};
use lsp_types::{PositionEncodingKind, SignatureHelpOptions};
use serde_json::{Value, json};

use super::{Signature, SignatureContext, SignatureHelp, SignatureTrigger};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{Clock, LspError, LspManager, ManualClock, ServerSpec, SystemClock};

const ROOT: &str = "/fake/root";

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

/// A UTF-16 server, with signature help options if `options`, and a buffer
/// of `text` open and synced.
fn started(options: Option<SignatureHelpOptions>, text: &str) -> (FakeServer, LspManager, Buffer) {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities.signature_help_provider = options;
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    });
    let clock = Arc::new(ManualClock::new());
    let mut manager = LspManager::with_clock(fake.launcher(), clock, |_| {});
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
            "text",
        )
        .unwrap();
    let uri = lsp_types::Url::from_file_path(&path).unwrap();
    fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    (fake, manager, buffer)
}

fn requests(fake: &FakeServer) -> Vec<Value> {
    (fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == SignatureHelpRequest::METHOD => {
                Some(params)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn labels_and_parameters() {
    let (fake, manager, buffer) = started(None, "f(");
    let ask = |reply: Value| -> Result<Option<SignatureHelp>, LspError> {
        fake.set_reply(
            SignatureHelpRequest::METHOD,
            Scripted::now(Reply::Result(reply)),
        );
        answer(manager.signature_help(
            buffer.id(),
            buffer.version(),
            2,
            SignatureContext::new(SignatureTrigger::Invoked),
        ))
    };

    // UTF-16 offsets into a label with a two-unit emoji (four bytes) and
    // an accented letter (two bytes); a string label found left to right.
    let label = "fn f(é: 🦀, b: u8)";
    let e = label.find('é').unwrap();
    let crab = label.find('🦀').unwrap();
    let b = label.find("b: u8").unwrap();
    let help = ask(json!({
        "signatures": [{
            "label": label,
            "documentation": { "kind": "markdown", "value": "*Calls* f." },
            "parameters": [
                // "é: 🦀" is units 5..9 (é 1, ':' 1, ' ' 1, 🦀 2 = 5 units).
                { "label": [5, 10] },
                { "label": "b: u8" },
            ],
        }],
        "activeSignature": 0,
        "activeParameter": 1,
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        help,
        SignatureHelp::new(
            vec![Signature::new(
                label,
                Some("*Calls* f.".into()),
                vec![e..crab + 4, b..b + 5],
                None,
            )],
            0,
            Some(1),
        )
    );

    // Strings are found left to right: the second "a" is after the first.
    let help = ask(json!({
        "signatures": [{
            "label": "g(a, aa)",
            "documentation": "plain",
            "parameters": [{ "label": "a" }, { "label": "a" }, { "label": "zz" }],
            "activeParameter": 7,
        }],
    }))
    .unwrap()
    .unwrap();
    assert_eq!(
        help.signatures[0],
        Signature::new(
            "g(a, aa)",
            Some("plain".into()),
            vec![2..3, 5..6, 8..8],
            Some(2)
        )
    );

    // Active ones out of range are clamped.
    let help = ask(json!({
        "signatures": [
            { "label": "h(x)", "parameters": [{ "label": [2, 3] }] },
            { "label": "h()", "parameters": [] },
        ],
        "activeSignature": 9,
        "activeParameter": 4,
    }))
    .unwrap()
    .unwrap();
    assert_eq!(help.active_signature, 1);
    assert_eq!(help.active_parameter, None);
    let help = ask(json!({
        "signatures": [{ "label": "h(x)", "parameters": [{ "label": [2, 3] }] }],
        "activeParameter": 4,
    }))
    .unwrap()
    .unwrap();
    assert_eq!((help.active_signature, help.active_parameter), (0, Some(0)));

    // No signatures, or no reply, is none.
    assert_eq!(ask(json!({ "signatures": [] })), Ok(None));
    assert_eq!(ask(Value::Null), Ok(None));
}

#[test]
fn context_and_triggers() {
    let (fake, manager, buffer) = started(
        Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![")".into()]),
            work_done_progress_options: Default::default(),
        }),
        "f(é, ",
    );
    assert_eq!(
        manager.signature_help_triggers(buffer.id()),
        Some((vec!['(', ','], vec![')']))
    );

    fake.set_reply(
        SignatureHelpRequest::METHOD,
        Scripted::now(Reply::Result(Value::Null)),
    );
    let at = "f(é, ".len();
    let ask = |context: SignatureContext| {
        answer(manager.signature_help(buffer.id(), buffer.version(), at, context)).unwrap();
        requests(&fake).pop().unwrap()
    };

    let params = ask(SignatureContext::new(SignatureTrigger::Invoked));
    assert_eq!(params["position"], json!({ "line": 0, "character": 5 }));
    assert_eq!(params["context"]["triggerKind"], json!(1));
    assert_eq!(params["context"]["isRetrigger"], json!(false));
    assert!(params["context"].get("triggerCharacter").is_none());
    assert!(params["context"].get("activeSignatureHelp").is_none());

    // The help showing goes along, its parameters as UTF-16 offsets.
    let active = SignatureHelp::new(
        vec![Signature::new(
            "f(é, b)",
            Some("docs".into()),
            vec![2..4, 6..7],
            Some(1),
        )],
        0,
        Some(0),
    );
    let mut context = SignatureContext::new(SignatureTrigger::Character(','));
    context.is_retrigger = true;
    context.active = Some(active);
    let params = ask(context);
    assert_eq!(params["context"]["triggerKind"], json!(2));
    assert_eq!(params["context"]["triggerCharacter"], json!(","));
    assert_eq!(params["context"]["isRetrigger"], json!(true));
    assert_eq!(
        params["context"]["activeSignatureHelp"],
        json!({
            "signatures": [{
                "label": "f(é, b)",
                "documentation": "docs",
                "parameters": [{ "label": [2, 3] }, { "label": [5, 6] }],
                "activeParameter": 1,
            }],
            "activeSignature": 0,
            "activeParameter": 0,
        })
    );

    let params = ask(SignatureContext::new(SignatureTrigger::ContentChange));
    assert_eq!(params["context"]["triggerKind"], json!(3));

    // A server with no signature help has no triggers.
    let (_fake, manager, buffer) = started(None, "f(");
    assert_eq!(manager.signature_help_triggers(buffer.id()), None);
}

#[path = "signature_help_label_tests.rs"]
mod label_tests;

#[path = "signature_help_active_default_tests.rs"]
mod active_default_tests;
