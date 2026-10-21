//! Hover in the workbench (docs/decisions/lsp-client.md §7): each editor's
//! hover provider asks its buffer's server, and `cmd-k cmd-i` shows the
//! answer at the caret. A server that never answers shows nothing, and the
//! editor goes on as before.

use std::time::Duration;

use fe_lsp::fake::{Record, Reply, Scripted};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{
    LSPAny, LSPObject,
    request::{HoverRequest, Request as _},
};

use super::lsp::Lsp;

const DOCS: &str = "```rust\nfn main()\n```\nThe program's entry point.";

/// The hover requests the fake got: their params.
fn hovers(records: &[Record]) -> Vec<LSPAny> {
    (records.iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == HoverRequest::METHOD => {
                Some(params.clone())
            }
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn hover_from_the_server(cx: &mut TestAppContext) {
    // `{"contents": {"kind": "markdown", "value": DOCS}}`.
    let markup = LSPObject::from_iter([
        ("kind".to_owned(), LSPAny::from("markdown")),
        ("value".to_owned(), LSPAny::from(DOCS)),
    ]);
    let hover = LSPObject::from_iter([("contents".to_owned(), LSPAny::Object(markup))]);
    let mut s = Lsp::new(cx);
    s.fake.set_reply(
        HoverRequest::METHOD,
        Scripted::now(Reply::Result(LSPAny::Object(hover))),
    );
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    // The caret in `main`, the editor focused.
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn ma".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();

    // Asked about the caret, in the open file; its markdown is shown.
    s.cx.simulate_keystrokes("cmd-k cmd-i");
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });
    let shown = editor.read_with(&s.cx, |view, _| view.popover_text());
    assert_eq!(
        shown.as_deref(),
        Some("fn main()\nThe program's entry point.")
    );
    let asked = hovers(&s.fake.records());
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["textDocument"]["uri"].as_str(),
        Some(main.as_str())
    );
    let position = &asked[0]["position"];
    assert_eq!(
        (position["line"].as_u64(), position["character"].as_u64()),
        (Some(0), Some(5))
    );
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert_eq!(editor.read_with(&s.cx, |view, _| view.popover_text()), None);

    // A server that never answers: nothing shows, and typing goes on.
    s.fake
        .set_reply(HoverRequest::METHOD, Scripted::now(Reply::Never));
    s.cx.simulate_keystrokes("cmd-k cmd-i");
    s.draw();
    let fake = s.fake.clone();
    fake.wait_for("the second hover", |records| hovers(records).len() == 2);
    // Past `lsp.timeouts.hover`'s 3 s it's cancelled, still showing
    // nothing.
    s.clock.wait_for_sleep(Duration::from_secs(3));
    s.clock.advance(Duration::from_secs(3));
    // The timeout lands on the workbench's executor, which drawing runs.
    s.until("$/cancelRequest", |s| {
        (s.fake.records().iter()).any(|record| record.is_notification("$/cancelRequest"))
    });
    assert_eq!(editor.read_with(&s.cx, |view, _| view.popover_text()), None);
    // Typing goes on, and reaches the server.
    s.cx.simulate_input("x");
    s.draw();
    let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    assert_eq!(text, "fn maxin() {}\n");
    s.fake.wait_for_document(&main, "the edit", |r| {
        r.is_some_and(|r| r.text == "fn maxin() {}\n")
    });
}

#[path = "lsp_inlay_hints.rs"]
mod inlay_hints;
