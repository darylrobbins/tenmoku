//! Code actions on the fake server: what a request carries, the order of
//! a reply, resolving, decoding and running, and the timeout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::{Buffer, BufferId};
use futures::FutureExt;
use lsp_types::{
    CodeActionOptions, CodeActionProviderCapability, ExecuteCommandOptions, PositionEncodingKind,
};
use serde_json::{Value, json};

use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{
    CodeAction, Diagnostic, DiagnosticTag, EditTarget, LspError, LspManager, ManualClock, ServerId,
    ServerSpec, Severity, SystemClock,
};

const ROOT: &str = "/fake/root";
const ACTION: &str = "textDocument/codeAction";
const RESOLVE: &str = "codeAction/resolve";
const EXECUTE: &str = "workspace/executeCommand";

struct Started {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    server: ServerId,
    buffer: Buffer,
    uri: lsp_types::Url,
}

/// The document: an emoji (two UTF-16 units, four bytes) before `beta`.
const TEXT: &str = "let 🦀 = alpha.beta();\n";

impl Started {
    fn new(resolves: bool) -> Self {
        let mut result = initialize_result(Some(PositionEncodingKind::UTF16));
        result.capabilities.code_action_provider =
            Some(CodeActionProviderCapability::Options(CodeActionOptions {
                resolve_provider: Some(resolves),
                ..Default::default()
            }));
        result.capabilities.execute_command_provider = Some(ExecuteCommandOptions {
            commands: vec!["fix.all".into()],
            ..Default::default()
        });
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
            clock,
            manager,
            server,
            buffer,
            uri,
        }
    }

    fn id(&self) -> BufferId {
        self.buffer.id()
    }

    fn ask(&self, diagnostics: Vec<Diagnostic>) -> Result<Vec<CodeAction>, LspError> {
        let beta = TEXT.find("beta").unwrap();
        answer(self.manager.code_actions(
            self.id(),
            self.buffer.version(),
            beta..beta + 4,
            diagnostics,
        ))
    }

    fn reply(&self, method: &str, reply: Value) {
        self.fake
            .set_reply(method, Scripted::now(Reply::Result(reply)));
    }

    /// The params of each request of `method` the fake got.
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

fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout =
        futures::FutureExt::fuse(crate::Clock::sleep(&SystemClock, Duration::from_secs(10)));
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in 10s"),
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

fn range(range: std::ops::Range<usize>) -> Value {
    json!({ "start": position(range.start), "end": position(range.end) })
}

fn titles(actions: &[CodeAction]) -> Vec<&str> {
    actions.iter().map(|action| action.title.as_str()).collect()
}

#[test]
fn request_and_order() {
    let s = Started::new(false);
    let beta = TEXT.find("beta").unwrap();
    let diagnostic = lsp_diagnostic(beta);
    s.reply(
        ACTION,
        json!([
            { "title": "Run lint", "command": "lint.run", "arguments": [1] },
            { "title": "Extract", "kind": "refactor.extract" },
            { "title": "Fix B", "kind": "quickfix", "diagnostics": [diagnostic] },
            { "title": "Fix A", "kind": "quickfix.import", "isPreferred": true,
              "diagnostics": [diagnostic] },
            { "title": "Inline", "kind": "refactor.inline",
              "disabled": { "reason": "not here" } },
        ]),
    );
    let mut given = lsp_to_fe(beta);
    given.data = Some(json!({ "id": 7 }));
    let actions = s.ask(vec![given]).unwrap();

    // The range and the diagnostic, in UTF-16, with its data.
    let sent = s.sent(ACTION);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["textDocument"]["uri"], json!(s.uri));
    assert_eq!(sent[0]["range"], range(beta..beta + 4));
    assert_eq!(sent[0]["context"]["diagnostics"], json!([diagnostic]));

    assert_eq!(
        titles(&actions),
        ["Fix A", "Fix B", "Run lint", "Extract", "Inline"]
    );
    assert!(actions[0].is_preferred && actions[0].is_quick_fix());
    assert_eq!(actions[0].diagnostics_fixed, 1);
    assert_eq!(actions[2].kind, None);
    assert_eq!(actions[2].diagnostics_fixed, 0);
    assert_eq!(actions[4].disabled.as_deref(), Some("not here"));
}

fn lsp_diagnostic(beta: usize) -> Value {
    json!({
        "range": range(beta..beta + 4),
        "severity": 1,
        "code": 308,
        "source": "tsc",
        "message": "no beta",
        "tags": [1],
        "data": { "id": 7 },
    })
}

fn lsp_to_fe(beta: usize) -> Diagnostic {
    let mut diagnostic = crate::diagnostics::decode(
        &Buffer::new(TEXT).text().clone(),
        crate::PositionEncoding::Utf16,
        vec![serde_json::from_value(lsp_diagnostic(beta)).unwrap()],
    )
    .remove(0);
    assert_eq!(diagnostic.severity, Some(Severity::Error));
    assert_eq!(diagnostic.tags, [DiagnosticTag::Unnecessary]);
    diagnostic.data = None;
    diagnostic
}

#[test]
fn resolve_edits_and_commands() {
    let s = Started::new(true);
    let beta = TEXT.find("beta").unwrap();
    s.reply(
        ACTION,
        json!([
            { "title": "Lazy", "kind": "quickfix", "data": { "token": 3 },
              "command": { "title": "Lazy", "command": "fix.all", "arguments": ["x", 2] } },
            { "title": "Eager", "kind": "quickfix",
              "edit": { "changes": { s.uri.as_str(): [
                  { "range": range(beta..beta + 4), "newText": "gamma" }] } } },
        ]),
    );
    let actions = s.ask(vec![]).unwrap();
    assert_eq!(titles(&actions), ["Lazy", "Eager"]);
    assert!(s.manager.code_action_edits(&actions[0]).unwrap().is_empty());

    // Eager has its edit, so it isn't resolved.
    let eager = answer(s.manager.resolve_code_action(&actions[1])).unwrap();
    assert!(s.sent(RESOLVE).is_empty());
    let edits = s.manager.code_action_edits(&eager).unwrap();
    assert_eq!(edits.len(), 1);
    match &edits[0].target {
        EditTarget::Open { buffer, edits, .. } => {
            assert_eq!(*buffer, s.id());
            assert_eq!(edits, &[(beta..beta + 4, "gamma".to_owned())]);
        }
        other => panic!("{other:?}"),
    }

    // Lazy is, with its data, and its edit decodes.
    s.reply(
        RESOLVE,
        json!({ "title": "Lazy", "kind": "quickfix", "data": { "token": 3 },
            "edit": { "changes": { s.uri.as_str(): [
                { "range": range(beta..beta), "newText": "z" }] } } }),
    );
    let lazy = answer(s.manager.resolve_code_action(&actions[0])).unwrap();
    let sent = s.sent(RESOLVE);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["data"], json!({ "token": 3 }));
    match &s.manager.code_action_edits(&lazy).unwrap()[0].target {
        EditTarget::Open { edits, .. } => assert_eq!(edits, &[(beta..beta, "z".to_owned())]),
        other => panic!("{other:?}"),
    }

    // Running sends its command, with its arguments.
    s.reply(EXECUTE, Value::Null);
    answer(s.manager.run_code_action_command(&lazy)).unwrap();
    assert_eq!(
        s.sent(EXECUTE),
        [json!({ "command": "fix.all", "arguments": ["x", 2] })]
    );

    // An action with no command sends nothing; a disabled one isn't run.
    answer(s.manager.run_code_action_command(&eager)).unwrap();
    let mut disabled = lazy.clone();
    disabled.disabled = Some("no".into());
    assert!(answer(s.manager.run_code_action_command(&disabled)).is_err());
    assert_eq!(s.sent(EXECUTE).len(), 1);
}

#[test]
fn resolve_needs_the_capability() {
    let s = Started::new(false);
    s.reply(ACTION, json!([{ "title": "Lazy", "data": 1 }]));
    let actions = s.ask(vec![]).unwrap();
    answer(s.manager.resolve_code_action(&actions[0])).unwrap();
    assert!(s.sent(RESOLVE).is_empty());
}

#[test]
fn timeout_is_empty() {
    let s = Started::new(false);
    s.fake.set_reply(
        ACTION,
        Scripted {
            reply: Reply::Result(json!([{ "title": "Late" }])),
            delay: Duration::from_secs(60),
        },
    );
    let beta = TEXT.find("beta").unwrap();
    let mut late = s
        .manager
        .code_actions(s.id(), s.buffer.version(), beta..beta, vec![])
        .boxed();
    let records = s.fake.wait_for("the request", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { method, .. } if method == ACTION))
    });
    let id = records
        .iter()
        .find_map(|r| match r {
            Record::Request { id, method, .. } if method == ACTION => Some(id.clone()),
            _ => None,
        })
        .unwrap();
    s.clock.wait_for_sleep(Duration::from_secs(10));
    s.clock.advance(Duration::from_millis(9999));
    std::thread::sleep(Duration::from_millis(50));
    assert!(late.as_mut().now_or_never().is_none());
    s.clock.advance(Duration::from_millis(1));
    assert_eq!(answer(late).unwrap().len(), 0);
    s.fake.wait_for("$/cancelRequest", |records| {
        records.iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == json!(id))
        })
    });
}

#[test]
fn quick_fixes_for_the_given_diagnostics_rank_first() {
    let s = Started::new(false);
    let beta = TEXT.find("beta").unwrap();
    let diagnostic = lsp_diagnostic(beta);
    let other = json!({ "range": range(0..3), "message": "elsewhere" });
    s.reply(
        ACTION,
        json!([
            { "title": "Unrelated", "kind": "quickfix", "isPreferred": true,
              "diagnostics": [other] },
            { "title": "Related", "kind": "quickfix", "diagnostics": [diagnostic] },
            { "title": "Refactor", "kind": "refactor", "isPreferred": true },
        ]),
    );
    let actions = s.ask(vec![lsp_to_fe(beta)]).unwrap();
    assert_eq!(titles(&actions), ["Related", "Unrelated", "Refactor"]);
}

#[test]
fn bare_commands_are_not_resolved_and_stale_actions_not_sent() {
    let mut s = Started::new(true);
    s.reply(
        ACTION,
        json!([
            { "title": "Bare", "command": "fix.all" },
            { "title": "Lazy", "data": 1,
              "command": { "title": "Lazy", "command": "fix.all" } },
        ]),
    );
    let actions = s.ask(vec![]).unwrap();
    s.reply(EXECUTE, Value::Null);
    let bare = answer(s.manager.resolve_code_action(&actions[0])).unwrap();
    assert!(s.sent(RESOLVE).is_empty());
    answer(s.manager.run_code_action_command(&bare)).unwrap();
    assert_eq!(s.sent(EXECUTE).len(), 1);

    // The document is closed and opened again: the actions are from
    // before, and neither request goes out.
    s.manager.close(s.id());
    let mut buffer = Buffer::new(TEXT);
    let path = PathBuf::from(ROOT).join("a.rs");
    let subscription = buffer.subscribe();
    s.manager
        .open_on(
            s.server,
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            &path,
            "text",
        )
        .unwrap();
    s.buffer = buffer;
    s.fake
        .wait_for_document(&s.uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let stale = answer(s.manager.resolve_code_action(&actions[1])).unwrap();
    assert!(s.sent(RESOLVE).is_empty());
    assert!(matches!(
        answer(s.manager.run_code_action_command(&stale)),
        Err(LspError::Unavailable(_))
    ));
    assert_eq!(s.sent(EXECUTE).len(), 1);
}
