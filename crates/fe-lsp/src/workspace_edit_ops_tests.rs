//! Resource operations in workspace edits: decoded in order with their
//! options, and what refuses them.

use fe_buffer::Buffer;
use serde_json::json;

use super::{Started, assert_refused, path, text_edit, uri};
use crate::EditTarget;

#[test]
fn resource_operations_decode_in_order() {
    let mut s = Started::new();
    let text = "alpha beta gamma\n";
    let mut buffer = Buffer::new("");
    let open = s.open(&mut buffer, "a.rs", text);
    let version = s.version(&open);
    let decoded = s
        .decode(json!({ "documentChanges": [
            { "textDocument": { "uri": uri("a.rs"), "version": version },
              "edits": [text_edit(text, 0..5, "delta")] },
            { "kind": "create", "uri": uri("b.rs"), "options": { "overwrite": true } },
            { "textDocument": { "uri": uri("b.rs"), "version": null },
              "edits": [text_edit("", 0..0, "fn b() {}")] },
            { "kind": "rename", "oldUri": uri("c.rs"), "newUri": uri("d.rs"),
              "options": { "ignoreIfExists": true } },
            { "textDocument": { "uri": uri("d.rs"), "version": null },
              "edits": [text_edit("", 0..0, "fn d() {}")] },
            { "kind": "delete", "uri": uri("e.rs"), "options": { "recursive": true } },
        ]}))
        .unwrap();
    let paths: Vec<_> = decoded.iter().map(|file| file.path.clone()).collect();
    assert_eq!(
        paths,
        ["a.rs", "b.rs", "b.rs", "c.rs", "d.rs", "e.rs"].map(path)
    );
    assert!(matches!(decoded[0].target, EditTarget::Open { .. }));
    assert_eq!(
        decoded[1].target,
        EditTarget::Create {
            overwrite: true,
            ignore_if_exists: false
        }
    );
    assert!(matches!(decoded[2].target, EditTarget::Closed { .. }));
    assert_eq!(
        decoded[3].target,
        EditTarget::Rename {
            to: path("d.rs"),
            overwrite: false,
            ignore_if_exists: true
        }
    );
    assert!(matches!(decoded[4].target, EditTarget::Closed { .. }));
    assert_eq!(
        decoded[5].target,
        EditTarget::Delete {
            recursive: true,
            ignore_if_not_exists: false
        }
    );
}

#[test]
fn edits_after_a_delete_refuse_the_edit() {
    let s = Started::new();
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "delete", "uri": uri("e.rs") },
            { "textDocument": { "uri": uri("e.rs"), "version": null },
              "edits": [text_edit("", 0..0, "x")] },
        ]})),
        "e.rs",
    );
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "create", "uri": "http://example.com/x" },
        ]})),
        "isn't a file",
    );
}
