//! Items a server sends as snippets (`insertTextFormat` 2) are inserted as
//! the text they expand to, until snippet completions land.

use lsp_types::PositionEncodingKind;
use lsp_types::request::{Completion, Request};
use serde_json::{Value, json};

use super::super::answer;
use super::{edit, started};
use crate::CompletionTrigger;
use crate::fake::{Reply, Scripted};
use fe_buffer::Buffer;

/// The edits of the items the fake lists, asked at the end of `fo` in
/// `text`, with the range of `fo`.
fn edits(items: Value) -> (Vec<crate::CompletionEdit>, std::ops::Range<usize>) {
    let (mut s, _) = started(PositionEncodingKind::UTF8);
    let text = "let x = fo\n";
    let mut buffer = Buffer::new(text);
    s.open_synced(&mut buffer, "a.rs");
    let end = text.len() - 1;
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(items)));
    let items = answer(s.manager.completion(
        buffer.id(),
        buffer.version(),
        end,
        CompletionTrigger::Invoked,
    ))
    .unwrap()
    .unwrap()
    .items;
    (
        items.into_iter().map(|item| item.edit).collect(),
        end - 2..end,
    )
}

#[test]
fn snippet_items_insert_their_plain_text() {
    let range =
        json!({ "start": { "line": 0, "character": 8 }, "end": { "line": 0, "character": 10 } });
    let (got, word) = edits(json!([
        {
            "label": "foo", "insertTextFormat": 2,
            "textEdit": { "range": range, "newText": "foo(${1:a}, ${2:b})$0" },
        },
        { "label": "y", "insertTextFormat": 2, "insertText": "${1|x,y|} = ${TM_FILENAME:f}\\$" },
        { "label": "o", "insertTextFormat": 2, "insertText": "${1:outer ${2:inner}}" },
        { "label": "bar", "insertTextFormat": 1, "insertText": "bar($1)" },
    ]));
    assert_eq!(
        got,
        [
            edit(word.clone(), "foo(a, b)"),
            edit(word.clone(), "x = f$"),
            edit(word.clone(), "outer inner"),
            edit(word, "bar($1)"),
        ]
    );
}

#[test]
fn malformed_snippets_stay_literal() {
    let (got, word) = edits(json!([
        { "label": "a", "insertTextFormat": 2, "insertText": "a${1:b" },
        { "label": "c", "insertTextFormat": 2, "insertText": "cost: $" },
        { "label": "d", "insertTextFormat": 2, "insertText": "${1:ok} a${2:b" },
    ]));
    assert_eq!(
        got,
        [
            edit(word.clone(), "a${1:b"),
            edit(word.clone(), "cost: $"),
            edit(word, "ok a${2:b"),
        ]
    );
}

#[test]
fn deep_nesting_stays_literal_without_overflowing() {
    let deep = "${1:".repeat(100_000);
    let (got, word) = edits(json!([
        { "label": "e", "insertTextFormat": 2, "insertText": deep },
    ]));
    assert_eq!(got, [edit(word, &deep)]);
}
