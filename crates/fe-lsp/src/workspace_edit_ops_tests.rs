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

#[test]
fn a_version_on_a_fresh_path_is_refused() {
    let s = Started::new();
    let edit = |name: &str, version: serde_json::Value| {
        json!({ "textDocument": { "uri": uri(name), "version": version },
                "edits": [text_edit("", 0..0, "x")] })
    };
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "create", "uri": uri("b.rs") },
            edit("b.rs", json!(7)),
        ]})),
        "b.rs",
    );
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "rename", "oldUri": uri("c.rs"), "newUri": uri("d.rs") },
            { "textDocument": { "uri": uri("d.rs"), "version": 3 }, "edits": [] },
        ]})),
        "d.rs",
    );
    let decoded = s
        .decode(json!({ "documentChanges": [
            { "kind": "create", "uri": uri("b.rs") },
            edit("b.rs", json!(null)),
        ]}))
        .unwrap();
    assert!(matches!(decoded[1].target, EditTarget::Closed { .. }));
}

#[test]
fn directory_operations_cover_the_paths_under_them() {
    let mut s = Started::new();
    let edit = |name: &str| {
        json!({ "textDocument": { "uri": uri(name), "version": null },
                "edits": [text_edit("", 0..0, "x")] })
    };
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "delete", "uri": uri("a") },
            edit("a/x.rs"),
        ]})),
        "a/x.rs",
    );
    assert_refused(
        s.decode(json!({ "documentChanges": [
            { "kind": "rename", "oldUri": uri("a"), "newUri": uri("b") },
            edit("a/x.rs"),
        ]})),
        "a/x.rs",
    );
    let mut buffer = Buffer::new("");
    s.open(&mut buffer, "b/x.rs", "text\n");
    let decoded = s
        .decode(json!({ "documentChanges": [
            { "kind": "rename", "oldUri": uri("a"), "newUri": uri("b") },
            edit("b/x.rs"),
        ]}))
        .unwrap();
    assert!(matches!(decoded[1].target, EditTarget::Closed { .. }));
    let decoded = s
        .decode(json!({ "documentChanges": [
            { "kind": "delete", "uri": uri("a") },
            { "kind": "create", "uri": uri("a/x.rs") },
            edit("a/x.rs"),
        ]}))
        .unwrap();
    assert!(matches!(decoded[2].target, EditTarget::Closed { .. }));
    let decoded = s
        .decode(json!({ "documentChanges": [
            { "kind": "delete", "uri": uri("a/b") },
            edit("a/bc"),
        ]}))
        .unwrap();
    assert!(matches!(decoded[1].target, EditTarget::Closed { .. }));
}

/// Two equal renames `ignoreIfExists` may skip are two operations: the
/// edit after them names the first one's outcome apart from the second's.
#[test]
fn repeated_skippable_renames_have_distinct_identities() {
    let mut s = Started::new();
    let mut a = Buffer::new("");
    s.open(&mut a, "a.rs", "alpha beta\n");
    let rename = json!({ "kind": "rename", "oldUri": uri("a.rs"), "newUri": uri("b.rs"),
                         "options": { "ignoreIfExists": true } });
    let decoded = s
        .decode(json!({ "documentChanges": [
            rename,
            { "kind": "create", "uri": uri("a.rs") },
            rename,
            { "textDocument": { "uri": uri("b.rs"), "version": null },
              "edits": [text_edit("alpha beta\n", 6..10, "gamma")] },
        ]}))
        .unwrap();
    let (first, second) = (decoded[0].operation, decoded[2].operation);
    assert!(first.is_some() && second.is_some());
    assert_ne!(first, second);
    assert!(decoded[1].operation.is_some());
    // The rename made at the second is skipped (b.rs is there by then): the
    // edit's chain names the second, and under it the first.
    let EditTarget::Depends { on, made, skipped } = &decoded[3].target else {
        panic!("{:?}", decoded[3]);
    };
    assert_eq!(Some(*on), second);
    assert!(
        matches!(&**made, Ok(EditTarget::Depends { on, .. }) if Some(*on) == first)
            || matches!(&**skipped, Ok(EditTarget::Depends { on, .. }) if Some(*on) == first)
    );
}
