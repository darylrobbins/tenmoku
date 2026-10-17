//! A resolve's `additionalTextEdits` (a TypeScript auto-import) are kept,
//! as bytes of the version the item was listed at.

use lsp_types::PositionEncodingKind;
use lsp_types::request::{Completion, Request, ResolveCompletionItem};
use serde_json::json;

use super::super::answer;
use super::{edit, started};
use crate::CompletionTrigger;
use crate::fake::{Reply, Scripted};
use fe_buffer::Buffer;

fn item(
    s: &mut super::Started,
    buffer: &Buffer,
    offset: usize,
    listed: serde_json::Value,
) -> crate::CompletionItem {
    s.fake.set_reply(
        Completion::METHOD,
        Scripted::now(Reply::Result(json!([listed]))),
    );
    answer(s.manager.completion(
        buffer.id(),
        buffer.version(),
        offset,
        CompletionTrigger::Invoked,
    ))
    .unwrap()
    .unwrap()
    .items
    .remove(0)
}

#[test]
fn resolve_keeps_the_additional_edits_it_resolves() {
    let (mut s, _) = started(PositionEncodingKind::UTF16);
    // Row 1 has an emoji (two UTF-16 units, four bytes) before the word.
    let text = "import a;\nlet 🦀 = fo\n";
    let mut buffer = Buffer::new(text);
    let uri = s.open_synced(&mut buffer, "a.ts");
    let end = text.len() - 1;
    let main = json!({
        "range": { "start": { "line": 1, "character": 11 }, "end": { "line": 1, "character": 13 } },
        "newText": "foo",
    });
    let item = item(
        &mut s,
        &buffer,
        end,
        json!({ "label": "foo", "textEdit": main }),
    );
    assert!(item.additional_edits.is_empty());

    // The buffer moves on before the resolve.
    s.edit(&mut buffer, &uri, (0..0, "// c\n"), 1);

    s.fake.set_reply(
        ResolveCompletionItem::METHOD,
        Scripted::now(Reply::Result(json!({
            "label": "foo",
            "textEdit": main,
            "additionalTextEdits": [
                {
                    "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                    "newText": "import { foo } from \"./foo\";\n",
                },
                {
                    "range": { "start": { "line": 1, "character": 6 }, "end": { "line": 1, "character": 7 } },
                    "newText": "",
                },
            ],
        }))),
    );
    let resolved = answer(s.manager.resolve(item.clone())).unwrap();
    // "import a;\n" is 10 bytes; "let " and the emoji are 8 more.
    assert_eq!(
        resolved.additional_edits,
        [
            edit(0..0, "import { foo } from \"./foo\";\n"),
            edit(18..19, "")
        ]
    );
    assert_eq!(resolved.edit, item.edit);
}

#[test]
fn a_resolve_without_edits_keeps_the_listed_ones() {
    let (mut s, _) = started(PositionEncodingKind::UTF16);
    let text = "import a;\nlet x = fo\n";
    let mut buffer = Buffer::new(text);
    s.open_synced(&mut buffer, "a.ts");
    let item = item(
        &mut s,
        &buffer,
        text.len() - 1,
        json!({
            "label": "foo",
            "additionalTextEdits": [{
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                "newText": "import b;\n",
            }],
        }),
    );
    assert_eq!(item.additional_edits, [edit(0..0, "import b;\n")]);

    s.fake.set_reply(
        ResolveCompletionItem::METHOD,
        Scripted::now(Reply::Result(json!({ "label": "foo", "detail": "d" }))),
    );
    let resolved = answer(s.manager.resolve(item.clone())).unwrap();
    assert_eq!(resolved.detail.as_deref(), Some("d"));
    assert_eq!(resolved.additional_edits, item.additional_edits);
}
