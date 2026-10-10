//! Completion in the workbench (docs/decisions/lsp-client.md §7): each
//! editor's completion provider asks its buffer's server, and accepting an
//! item applies its edit and its import as one undoable edit.

use fe_lsp::fake::{Record, Reply, Scripted};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{
    LSPAny,
    request::{Completion, Request as _},
};

use super::lsp::Lsp;

#[gpui::test]
fn complete_from_the_server(cx: &mut TestAppContext) {
    // `main` becomes `main_loop`, and `use std::io;` goes at the top.
    let reply: LSPAny = r#"{
        "isIncomplete": false,
        "items": [{
            "label": "main_loop",
            "kind": 3,
            "textEdit": {
                "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 7 } },
                "newText": "main_loop"
            },
            "additionalTextEdits": [{
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                "newText": "use std::io;\n"
            }]
        }]
    }"#
    .parse()
    .unwrap();
    let mut s = Lsp::new(cx);
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(reply)));
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    // The caret at the end of `main`, the editor focused.
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn main".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();

    // Asked at the caret, in the open file; the menu shows its item.
    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    let asked: Vec<LSPAny> = (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == Completion::METHOD => {
                Some(params.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0]["textDocument"]["uri"].as_str(),
        Some(main.as_str())
    );
    let position = &asked[0]["position"];
    assert_eq!(
        (position["line"].as_u64(), position["character"].as_u64()),
        (Some(0), Some(7))
    );

    // `enter` inserts the item and its import, which reach the server.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    let text = |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    let accepted = "use std::io;\nfn main_loop() {}\n";
    assert_eq!(text(&mut s), accepted);
    assert!(!editor.read_with(&s.cx, |view, _| view.completion_shown()));
    s.fake.wait_for_document(&main, "the completion", |r| {
        r.is_some_and(|r| r.text == accepted)
    });

    // One `cmd-z` takes both back.
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&mut s), "fn main() {}\n");
    s.fake.wait_for_document(&main, "the undo", |r| {
        r.is_some_and(|r| r.text == "fn main() {}\n")
    });
}
