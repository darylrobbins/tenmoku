//! `textDocument/semanticTokens/full` and `full/delta`: tokens decoded
//! through the legend in bytes of the version, in a UTF-8 and a UTF-16
//! session, a delta applied to the previous stream, a server without the
//! provider, and the capabilities advertised.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use futures::FutureExt;
use lsp_types::request::{Request, SemanticTokensFullDeltaRequest, SemanticTokensFullRequest};
use lsp_types::{
    PositionEncodingKind, SemanticTokenModifier, SemanticTokenType, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensServerCapabilities,
    ServerCapabilities, Url,
};
use serde_json::{Value, json};

use super::super::{Started, answer, manager};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{LspError, ManualClock, SemanticToken, SemanticTokens, ServerSpec, ServerStatus};

struct Tokens {
    s: Started,
    clock: Arc<ManualClock>,
    buffer: Buffer,
    uri: Url,
    utf16: bool,
}

fn start(encoding: PositionEncodingKind, full: Option<SemanticTokensFullOptions>) -> Tokens {
    let utf16 = encoding == PositionEncodingKind::UTF16;
    let mut init = initialize_result(Some(encoding));
    init.capabilities = ServerCapabilities {
        semantic_tokens_provider: full.map(|full| {
            SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: vec![
                        SemanticTokenType::NAMESPACE,
                        SemanticTokenType::FUNCTION,
                        SemanticTokenType::PARAMETER,
                    ],
                    token_modifiers: vec![
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::DEFAULT_LIBRARY,
                    ],
                },
                full: Some(full),
                ..Default::default()
            })
        }),
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
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            Path::new(super::super::ROOT),
        )
        .unwrap();
    events.status(server);
    fake.wait_initialized();
    let mut s = Started {
        fake,
        manager,
        server,
        events,
    };
    let mut buffer = Buffer::new(TEXT);
    let uri = s.open_synced(&mut buffer, "a.rs");
    Tokens {
        s,
        clock,
        buffer,
        uri,
        utf16,
    }
}

fn delta() -> Option<SemanticTokensFullOptions> {
    Some(SemanticTokensFullOptions::Delta { delta: Some(true) })
}

fn full_only() -> Option<SemanticTokensFullOptions> {
    Some(SemanticTokensFullOptions::Bool(true))
}

fn requests(records: &[Record]) -> Vec<(String, Value)> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. }
                if method.starts_with("textDocument/semantic") =>
            {
                Some((method.clone(), params.clone()))
            }
            _ => None,
        })
        .collect()
}

// An emoji before the tokens: four bytes, two UTF-16 units.
const TEXT: &str = "let 🦀 = run(arg);\nstd::fs::read(p);\n";

/// The (line, character) of byte `offset` of `TEXT`.
fn position(offset: usize, utf16: bool) -> (u32, u32) {
    let before = &TEXT[..offset];
    let line = before.matches('\n').count();
    let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
    let character = if utf16 {
        column.encode_utf16().count()
    } else {
        column.len()
    };
    (line as u32, character as u32)
}

/// One token of a stream: the word it covers, its type index and its
/// modifier bits.
type Raw = (&'static str, u32, u32);

/// `tokens`, in order, as the server's relative stream.
fn stream(tokens: &[Raw], utf16: bool) -> Vec<u32> {
    let (mut line, mut start) = (0, 0);
    let mut data = Vec::new();
    for &(word, kind, bits) in tokens {
        let (at_line, at) = position(TEXT.find(word).unwrap(), utf16);
        let delta_start = if at_line == line { at - start } else { at };
        // The words are ASCII: as long in either encoding.
        data.extend([at_line - line, delta_start, word.len() as u32, kind, bits]);
        (line, start) = (at_line, at);
    }
    data
}

const FIRST: [Raw; 4] = [
    ("run", 1, 0b1),
    ("arg", 2, 0),
    // Out of the legend: skipped, though the next token is placed from it.
    ("std", 9, 0),
    // Bit 4 is past the legend's modifiers.
    ("read", 1, 0b10010),
];

/// `FIRST` with its second token changed.
const EDITED: [Raw; 4] = [FIRST[0], ("arg", 0, 0b11), FIRST[2], FIRST[3]];

fn summary(tokens: &SemanticTokens) -> Vec<(&str, &str, Vec<&str>)> {
    (tokens.tokens.iter())
        .map(|token: &SemanticToken| {
            (
                &TEXT[token.range.clone()],
                &*token.token_type,
                token.modifiers.iter().map(|name| &**name).collect(),
            )
        })
        .collect()
}

fn reply(t: &Tokens, method: &str, reply: Value) {
    t.s.fake
        .set_reply(method, Scripted::now(Reply::Result(reply)));
}

fn ask(t: &Tokens, previous: Option<&SemanticTokens>) -> Option<SemanticTokens> {
    answer(
        t.s.manager
            .semantic_tokens(t.buffer.id(), t.buffer.version(), previous),
    )
    .unwrap()
}

#[test]
fn full_tokens_decode_through_the_legend() {
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let t = start(encoding.clone(), delta());
        reply(
            &t,
            SemanticTokensFullRequest::METHOD,
            json!({ "resultId": "1", "data": stream(&FIRST, t.utf16) }),
        );
        let found = ask(&t, None).unwrap();
        assert_eq!(
            summary(&found),
            vec![
                ("run", "function", vec!["declaration"]),
                ("arg", "parameter", vec![]),
                ("read", "function", vec!["defaultLibrary"]),
            ],
            "{encoding:?}"
        );
        let read = TEXT.find("read").unwrap();
        assert_eq!(found.tokens[2].range, read..read + 4);
        assert_eq!(found.result_id.as_deref(), Some("1"));
        let sent = requests(&t.s.fake.records());
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, SemanticTokensFullRequest::METHOD);
        assert_eq!(
            sent[0].1,
            json!({ "textDocument": { "uri": "file:///fake/root/a.rs" } })
        );
    }
}

#[test]
fn delta_edits_apply_to_the_previous_data() {
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        let t = start(encoding.clone(), delta());
        reply(
            &t,
            SemanticTokensFullRequest::METHOD,
            json!({ "resultId": "1", "data": stream(&FIRST, t.utf16) }),
        );
        let first = ask(&t, None).unwrap();

        // The second token's five integers replaced.
        let edited = stream(&EDITED, t.utf16);
        reply(
            &t,
            SemanticTokensFullDeltaRequest::METHOD,
            json!({
                "resultId": "2",
                "edits": [{ "start": 5, "deleteCount": 5, "data": edited[5..10] }],
            }),
        );
        let second = ask(&t, Some(&first)).unwrap();
        let (method, params) = requests(&t.s.fake.records()).pop().unwrap();
        assert_eq!(method, SemanticTokensFullDeltaRequest::METHOD);
        assert_eq!(params["previousResultId"], json!("1"));
        assert_eq!(second.result_id.as_deref(), Some("2"));

        // What a full reply of the edited stream gives.
        reply(
            &t,
            SemanticTokensFullRequest::METHOD,
            json!({ "resultId": "3", "data": edited }),
        );
        let full = ask(&t, None).unwrap();
        assert_eq!(second.tokens, full.tokens, "{encoding:?}");
        assert_eq!(
            summary(&second)[1],
            ("arg", "namespace", vec!["declaration", "defaultLibrary"])
        );

        // Edits of single integers inside a token, out of order: the
        // second token's modifiers, then its type, back to `FIRST`'s.
        reply(
            &t,
            SemanticTokensFullDeltaRequest::METHOD,
            json!({
                "resultId": "4",
                "edits": [
                    { "start": 9, "deleteCount": 1, "data": [0] },
                    { "start": 8, "deleteCount": 1, "data": [2] },
                ],
            }),
        );
        let third = ask(&t, Some(&second)).unwrap();
        let (_, params) = requests(&t.s.fake.records()).pop().unwrap();
        assert_eq!(params["previousResultId"], json!("2"));
        assert_eq!(third.tokens, first.tokens, "{encoding:?}");
        assert_eq!(third.result_id.as_deref(), Some("4"));
    }

    // A provider without delta: full again.
    let t = start(PositionEncodingKind::UTF16, full_only());
    reply(
        &t,
        SemanticTokensFullRequest::METHOD,
        json!({ "resultId": "1", "data": stream(&FIRST, t.utf16) }),
    );
    let first = ask(&t, None).unwrap();
    let second = ask(&t, Some(&first)).unwrap();
    assert_eq!(second.tokens, first.tokens);
    let methods: Vec<_> = (requests(&t.s.fake.records()).into_iter())
        .map(|(method, _)| method)
        .collect();
    assert_eq!(methods, vec![SemanticTokensFullRequest::METHOD; 2]);
}

#[test]
fn a_restart_asks_again_in_full() {
    let t = start(PositionEncodingKind::UTF16, delta());
    reply(
        &t,
        SemanticTokensFullRequest::METHOD,
        json!({ "resultId": "1", "data": stream(&FIRST, t.utf16) }),
    );
    let first = ask(&t, None).unwrap();

    // Asked while the new session initializes: nothing is sent, and once
    // it has, the caller is to ask again.
    t.s.fake.crash();
    while !matches!(t.s.events.status(t.s.server), ServerStatus::Restarting(_)) {}
    let mut early = (t.s.manager)
        .semantic_tokens(t.buffer.id(), t.buffer.version(), Some(&first))
        .boxed();
    assert!(early.as_mut().now_or_never().is_none());
    t.clock.wait_for_sleep(Duration::from_secs(1));
    t.clock.advance(Duration::from_secs(1));
    t.s.fake.wait_initialized_at(2);
    t.s.fake
        .wait_for_document(&t.uri, "reopened", |r| r.is_some_and(|r| r.launch == 2));
    // The wait polls every 20 ms of the manager's clock.
    t.clock.advance(Duration::from_millis(20));
    assert_eq!(answer(early), Err(LspError::Retry));
    assert!(requests(&t.s.fake.records_of(2)).is_empty());

    // The new session didn't make `first`: no delta from it.
    let again = ask(&t, Some(&first)).unwrap();
    assert_eq!(again.tokens, first.tokens);
    let methods: Vec<_> = (requests(&t.s.fake.records_of(2)).into_iter())
        .map(|(method, _)| method)
        .collect();
    assert_eq!(methods, [SemanticTokensFullRequest::METHOD]);
}

#[test]
fn another_opening_is_asked_in_full() {
    let mut t = start(PositionEncodingKind::UTF8, delta());
    // A document opened under a size limit.
    t.s.manager.set_max_file_bytes(TEXT.len() + 1);
    let mut buffer = Buffer::new(TEXT);
    t.uri = t.s.open_synced(&mut buffer, "b.rs");
    t.buffer = buffer;
    reply(
        &t,
        SemanticTokensFullRequest::METHOD,
        json!({ "resultId": "1", "data": stream(&FIRST, t.utf16) }),
    );
    let first = ask(&t, None).unwrap();
    let opens = |t: &Tokens| {
        (t.s.fake.records().iter())
            .filter(|record| record.method() == "textDocument/didOpen")
            .count()
    };

    // Past the size limit: closed. Asked with the edit back under it not
    // yet synced, it is opened again as the request is admitted: the
    // legend read when asked wasn't of that opening.
    let end = TEXT.len();
    (t.buffer.edit([(end..end, "too long")], Origin::User)).unwrap();
    t.s.manager.changed(t.buffer.id());
    t.s.fake
        .wait_for_document(&t.uri, "didClose", |r| r.is_some_and(|r| !r.open));
    (t.buffer.edit([(end..end + 8, "")], Origin::User)).unwrap();
    reply(
        &t,
        SemanticTokensFullDeltaRequest::METHOD,
        json!({ "resultId": "2", "edits": [{ "start": 8, "deleteCount": 1, "data": [0] }] }),
    );
    let reopened = (t.s.manager).semantic_tokens(t.buffer.id(), t.buffer.version(), Some(&first));
    assert_eq!(answer(reopened), Err(LspError::Retry));
    assert_eq!(opens(&t), 3);
    let methods: Vec<_> = (requests(&t.s.fake.records()).into_iter())
        .map(|(method, _)| method)
        .collect();
    assert_eq!(methods, [SemanticTokensFullRequest::METHOD; 2]);

    // Asked again: in full, as the opening isn't `first`'s.
    let again = ask(&t, Some(&first)).unwrap();
    assert_eq!(again.tokens, first.tokens);
    let (method, _) = requests(&t.s.fake.records()).pop().unwrap();
    assert_eq!(method, SemanticTokensFullRequest::METHOD);

    // And a delta from there on.
    ask(&t, Some(&again)).unwrap();
    let (method, params) = requests(&t.s.fake.records()).pop().unwrap();
    assert_eq!(method, SemanticTokensFullDeltaRequest::METHOD);
    assert_eq!(params["previousResultId"], json!("1"));
}

#[test]
fn no_provider_sends_nothing() {
    let t = start(PositionEncodingKind::UTF16, None);
    assert_eq!(ask(&t, None), None);
    assert!(requests(&t.s.fake.records()).is_empty());
}

#[test]
fn semantic_token_capabilities_are_advertised() {
    let t = start(PositionEncodingKind::UTF16, delta());
    let init = (t.s.fake.records().into_iter())
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        init["capabilities"]["textDocument"]["semanticTokens"],
        json!({
            "dynamicRegistration": false,
            "requests": { "full": { "delta": true } },
            "tokenTypes": [
                "namespace", "type", "class", "enum", "interface", "struct",
                "typeParameter", "parameter", "variable", "property", "enumMember",
                "event", "function", "method", "macro", "keyword", "modifier",
                "comment", "string", "number", "regexp", "operator", "decorator", "formatSpecifier", "escapeSequence",
            ],
            "tokenModifiers": [
                "declaration", "definition", "readonly", "static", "deprecated",
                "abstract", "async", "modification", "documentation", "defaultLibrary",
            ],
            "formats": ["relative"],
            "overlappingTokenSupport": false,
            "multilineTokenSupport": false,
            "augmentsSyntaxTokens": true,
        })
    );
    assert_eq!(
        init["capabilities"]["workspace"]["semanticTokens"],
        json!({ "refreshSupport": true })
    );
}
