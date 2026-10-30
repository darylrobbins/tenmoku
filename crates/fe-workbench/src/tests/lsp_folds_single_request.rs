//! One document version with no edits gets one foldingRange request, however
//! early the server has the document.

use std::time::Duration;

use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::TestAppContext;
use lsp_types::{FoldingRangeProviderCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::json;

use crate::tests::lsp::Lsp;

const FOLDING: &str = "textDocument/foldingRange";

#[gpui::test]
fn a_document_opened_while_the_server_starts_is_asked_for_once(cx: &mut TestAppContext) {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        ..init.capabilities
    };
    let rust = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        initialize_delay: Duration::from_secs(1),
        ..Script::default()
    };
    let mut s = Lsp::with_fake_clock(cx, rust, Script::default());
    s.fake.set_reply(
        FOLDING,
        Scripted {
            reply: Reply::Result(json!([{ "startLine": 1, "endLine": 3 }])),
            // Outlasts the clock advances below, so the reply stays pending.
            delay: Duration::from_secs(5),
        },
    );
    s.set_trust(true);
    std::fs::write(
        s.path("src/main.rs"),
        "fn a() {\n    one();\n    two();\n}\n",
    )
    .unwrap();
    // Drawn while the server starts, with no opening of the document yet.
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    // The clock moves until the server's start-up sleep ends, however late
    // it began.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !s.fake.document(&main).is_some_and(|replica| replica.open) {
        assert!(std::time::Instant::now() < deadline, "no didOpen");
        s.clock.advance(Duration::from_millis(250));
        std::thread::sleep(Duration::from_millis(5));
    }
    // Let a request sent while the server started reach it before the next
    // draw sees the opening, as it does when the machine is busy.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    while requests(&s) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    // Draw until the first request is recorded.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while requests(&s) == 0 {
        assert!(std::time::Instant::now() < deadline, "no request");
        s.draw();
        std::thread::sleep(Duration::from_millis(5));
    }
    // The reply is pending across these advances and draws.
    for _ in 0..10 {
        s.clock.advance(Duration::from_millis(100));
        s.draw();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(requests(&s), 1);
}

fn requests(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method, .. } if method == FOLDING))
        .count()
}
