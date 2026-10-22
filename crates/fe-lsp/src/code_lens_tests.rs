//! Code lenses on the fake server: decoding, resolving, running commands,
//! and the capabilities that ask for them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fe_buffer::Buffer;
use lsp_types::{CodeLensOptions, ExecuteCommandOptions, PositionEncodingKind};
use serde_json::{Value, json};

use super::{EXECUTE, ROOT, answer};
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{CodeLens, LspManager, ManualClock, ServerSpec};

const LENS: &str = "textDocument/codeLens";
const RESOLVE: &str = "codeLens/resolve";

/// Two rows, each an emoji (two UTF-16 units, four bytes) before `fn`.
const TEXT: &str = "🦀 fn a() {}\n🦀 fn b() {}\n";

struct Started {
    fake: FakeServer,
    manager: LspManager,
    buffer: Buffer,
}

impl Started {
    /// `provider`: whether it has a `codeLensProvider`, and then whether
    /// that resolves.
    fn new(provider: Option<bool>) -> Self {
        let mut result = initialize_result(Some(PositionEncodingKind::UTF16));
        result.capabilities.code_lens_provider = provider.map(|resolves| CodeLensOptions {
            resolve_provider: Some(resolves),
        });
        result.capabilities.execute_command_provider = Some(ExecuteCommandOptions {
            commands: vec!["lens.run".into()],
            ..Default::default()
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

    fn ask(&self) -> Option<Vec<CodeLens>> {
        answer(
            self.manager
                .code_lenses(self.buffer.id(), self.buffer.version()),
        )
        .unwrap()
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

/// The LSP range of `needle`'s `nth` occurrence in `TEXT`, in UTF-16.
fn range_of(needle: &str, nth: usize) -> Value {
    let start = TEXT.match_indices(needle).nth(nth).unwrap().0;
    let at = |offset: usize| {
        let before = &TEXT[..offset];
        let line = before.matches('\n').count();
        let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
        json!({ "line": line, "character": column.encode_utf16().count() })
    };
    json!({ "start": at(start), "end": at(start + needle.len()) })
}

fn bytes_of(needle: &str, nth: usize) -> std::ops::Range<usize> {
    let start = TEXT.match_indices(needle).nth(nth).unwrap().0;
    start..start + needle.len()
}

#[test]
fn lenses_decode_in_bytes_of_the_version() {
    let s = Started::new(Some(false));
    // The later row first.
    s.reply(
        LENS,
        json!([
            { "range": range_of("fn b", 0), "data": 2 },
            {
                "range": range_of("fn a", 0),
                "command": { "title": "1 reference", "command": "lens.run", "arguments": [1] },
            },
        ]),
    );
    let lenses = s.ask().unwrap();
    assert_eq!(lenses.len(), 2);
    assert_eq!(lenses[0].range, bytes_of("fn a", 0));
    assert_eq!(lenses[1].range, bytes_of("fn b", 0));
    let command = lenses[0].command.as_ref().unwrap();
    assert_eq!(command.title, "1 reference");
    assert_eq!(command.command, "lens.run");
    assert!(lenses[1].command.is_none());
    assert_eq!(s.sent(LENS).len(), 1);
}

#[test]
fn unresolved_lenses_resolve() {
    let s = Started::new(Some(true));
    s.reply(
        LENS,
        json!([{ "range": range_of("fn a", 0), "data": { "id": 7 } }]),
    );
    let lens = s.ask().unwrap().remove(0);
    assert!(lens.command.is_none());
    s.reply(
        RESOLVE,
        json!({
            "range": range_of("fn a", 0),
            "data": { "id": 7 },
            "command": { "title": "Run", "command": "lens.run" },
        }),
    );
    let resolved = answer(s.manager.resolve_code_lens(lens)).unwrap();
    assert_eq!(resolved.command.clone().unwrap().title, "Run");
    let sent = s.sent(RESOLVE);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["data"], json!({ "id": 7 }));

    // Resolved again, or complete from the start, it sends nothing more.
    let again = answer(s.manager.resolve_code_lens(resolved)).unwrap();
    assert_eq!(again.command.unwrap().title, "Run");
    s.reply(
        LENS,
        json!([{
            "range": range_of("fn a", 0),
            "command": { "title": "Done", "command": "lens.run" },
        }]),
    );
    let complete = s.ask().unwrap().remove(0);
    answer(s.manager.resolve_code_lens(complete)).unwrap();
    assert_eq!(s.sent(RESOLVE).len(), 1);

    // Without a resolveProvider, nothing is sent.
    let s = Started::new(Some(false));
    s.reply(LENS, json!([{ "range": range_of("fn a", 0), "data": 1 }]));
    let lens = s.ask().unwrap().remove(0);
    let same = answer(s.manager.resolve_code_lens(lens)).unwrap();
    assert!(same.command.is_none());
    assert!(s.sent(RESOLVE).is_empty());
}

#[test]
fn lens_commands_run_only_when_listed() {
    let s = Started::new(Some(false));
    s.reply(
        LENS,
        json!([
            {
                "range": range_of("fn a", 0),
                "command": { "title": "Run", "command": "lens.run", "arguments": ["x", 2] },
            },
            {
                "range": range_of("fn b", 0),
                "command": {
                    "title": "1 reference",
                    "command": "rust-analyzer.showReferences",
                    "arguments": [],
                },
            },
        ]),
    );
    s.reply(EXECUTE, Value::Null);
    let lenses = s.ask().unwrap();
    answer(s.manager.run_lens_command(&lenses[0])).unwrap();
    let sent = s.sent(EXECUTE);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["command"], json!("lens.run"));
    assert_eq!(sent[0]["arguments"], json!(["x", 2]));

    assert!(matches!(
        answer(s.manager.run_lens_command(&lenses[1])),
        Err(crate::LspError::Unavailable(_))
    ));
    assert_eq!(s.sent(EXECUTE).len(), 1);
}

#[test]
fn no_provider_sends_nothing() {
    let s = Started::new(None);
    assert!(s.ask().is_none());
    assert!(s.sent(LENS).is_empty());
}

#[test]
fn code_lens_capabilities_are_advertised() {
    let s = Started::new(None);
    let init = (s.fake.records().into_iter())
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    let capabilities = &init["capabilities"];
    assert_eq!(
        capabilities["textDocument"]["codeLens"],
        json!({ "dynamicRegistration": false })
    );
    assert_eq!(
        capabilities["workspace"]["codeLens"],
        json!({ "refreshSupport": true })
    );
    assert_eq!(
        capabilities["experimental"]["commands"]["commands"],
        json!([
            "editor.action.triggerParameterHints",
            "editor.action.triggerSuggest",
            "rust-analyzer.showReferences",
        ])
    );
}
