//! `workspace/{semanticTokens,inlayHint,codeLens}/refresh`: reported to the host.

use lsp_types::request::{
    CodeLensRefresh, InlayHintRefreshRequest, SemanticTokensRefresh, WorkspaceDiagnosticRefresh,
};
use serde_json::Value;

use crate::fake::{FakeServer, Script};
use crate::tests::manager;
use crate::{LspEvent, Refresh, ServerSpec};

#[test]
fn refresh_requests_are_reported() {
    let root = tempfile::tempdir().unwrap();
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), None);
    let server = manager
        .start(ServerSpec::new("refresh", "fake-server"), root.path())
        .unwrap();
    fake.wait_initialized();

    let asking = std::thread::spawn({
        let fake = fake.clone();
        move || {
            [
                fake.request::<SemanticTokensRefresh>(()),
                fake.request::<InlayHintRefreshRequest>(()),
                fake.request::<CodeLensRefresh>(()),
                fake.request::<WorkspaceDiagnosticRefresh>(()),
            ]
        }
    });
    for what in [
        Refresh::SemanticTokens,
        Refresh::InlayHints,
        Refresh::CodeLens,
    ] {
        let seen = events.until("a Refresh", |event| match event {
            LspEvent::Refresh { server, what } => Some((server, what)),
            _ => None,
        });
        assert_eq!(seen, (server, what));
    }
    for answer in asking.join().unwrap() {
        assert_eq!(answer, Ok(Value::Null));
    }
    // Answered in order, so every Refresh is in the channel by now: none more.
    assert!(
        !events
            .0
            .try_iter()
            .any(|event| matches!(event, LspEvent::Refresh { .. }))
    );
}
