//! Request admission by buffer version, the ring of versions sent, and the
//! size limit (§4.1).

use fe_buffer::{Buffer, Origin};
use serde_json::{Value, json};

use super::{Started, answer};
use crate::document::RING;
use crate::fake::{Echo, InitializeReply, Record, Script};
use crate::{Admitted, DEFAULT_MAX_FILE_BYTES, LspError, PositionEncoding};

/// The fake's `fake/echo` requests carrying `marker`.
fn echoes(s: &Started, marker: &str) -> usize {
    s.fake
        .records()
        .iter()
        .filter(|record| {
            matches!(record, Record::Request { method, params, .. }
                if method == "fake/echo" && params.get(marker).is_some())
        })
        .count()
}

fn unavailable(why: &str) -> Result<(Value, Admitted), LspError> {
    Err(LspError::Unavailable(why.into()))
}

#[test]
fn request_admission_by_version() {
    let mut s = Started::new(Script::default());
    let mut a = Buffer::new("one\n");
    let a_uri = s.open_synced(&mut a, "a.txt");
    let mut b = Buffer::new("two\n");
    let b_uri = s.open_synced(&mut b, "b.txt");
    s.edit(&mut b, &b_uri, (0..0, "2"), 1);

    // Naming the version not yet sent: drained first, then sent.
    a.edit([(0..0, "1")], Origin::User).unwrap();
    let v1 = a.version();
    let reply = s.manager.document_request::<Echo>(
        a.id(),
        v1,
        |admitted| json!({ "admitted": admitted.document.sent.lsp_version }),
    );
    let (result, admitted) = answer(reply).unwrap();
    assert_eq!(result.result["params"], json!({ "admitted": 1 }));
    let order: Vec<String> = s
        .fake
        .records()
        .into_iter()
        .filter_map(|record| match record {
            Record::Request { params, .. } if params.get("admitted").is_some() => {
                Some("request".into())
            }
            Record::Notification { method, params }
                if params["textDocument"]["uri"] == json!(a_uri) =>
            {
                Some(format!("{method} {}", params["textDocument"]["version"]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        [
            "textDocument/didOpen 0",
            "textDocument/didChange 1",
            "request"
        ]
    );
    assert_eq!(admitted.encoding, PositionEncoding::Utf8);
    let sent = &admitted.document;
    assert_eq!((sent.buffer, &sent.uri), (a.id(), &a_uri));
    assert_eq!(
        Some(sent.identity),
        s.manager.active_identity(a.id(), s.server)
    );
    assert_eq!(
        (sent.sent.lsp_version, sent.sent.version),
        (1, v1),
        "that version's ring entry"
    );
    assert_eq!(sent.sent.text.to_string(), "1one\n");
    assert_eq!(sent.sent.edits.len(), 1);
    assert_eq!(sent.sent.edits[0][0].new, 0..1);
    let [other] = &admitted.others[..] else {
        panic!("{:?}", admitted.others)
    };
    assert_eq!((other.buffer, &other.uri), (b.id(), &b_uri));
    assert_eq!(
        (other.sent.lsp_version, other.sent.version),
        (1, b.version())
    );
    assert_eq!(other.sent.text.to_string(), "2two\n");

    // Naming an older version: never reaches the fake.
    a.edit([(0..0, "2")], Origin::User).unwrap();
    let reply = s.manager.document_request::<Echo>(a.id(), v1, |_| {
        panic!("a request on a version the buffer has left was built")
    });
    assert_eq!(
        answer(reply).map(|_| ()),
        unavailable("buffer changed").map(|_| ())
    );
    s.barrier();
    assert_eq!(echoes(&s, "admitted"), 1);

    // The ring keeps the last 32 versions sent: the stale request's drain
    // sent version 2.
    let mut versions = Vec::new();
    for n in 0..40 {
        s.edit(&mut a, &a_uri, (0..0, "x"), 3 + n);
        versions.push((3 + n, a.version()));
    }
    let ring = answer(s.manager.ring(a.id()));
    assert_eq!(ring.len(), RING);
    assert_eq!(ring, versions[40 - RING..]);
    assert_eq!(ring[0].0, 11);
}

#[test]
fn large_documents_are_not_synced() {
    let mut s = Started::new(Script::default());
    assert_eq!(s.manager.max_file_bytes(), 52_428_800);
    assert_eq!(DEFAULT_MAX_FILE_BYTES, 52_428_800);
    s.manager.set_max_file_bytes(16);
    let too_large = || unavailable("too large for the language server").map(|_| ());

    // Over the limit from the start: never opened, even after more edits.
    let mut big = Buffer::new("seventeen bytes!\n");
    let big_uri = s.open(&mut big, "big.txt");
    big.edit([(0..0, "x")], Origin::User).unwrap();
    s.manager.changed(big.id());
    s.barrier();
    assert_eq!(s.methods(&big_uri), Vec::<String>::new());
    assert_eq!(s.manager.active_identity(big.id(), s.server), None);
    let reply =
        s.manager
            .document_request::<Echo>(big.id(), big.version(), |_| json!({ "big": true }));
    assert_eq!(answer(reply).map(|_| ()), too_large());

    // Grown past it: closed, and not opened again until back under.
    let mut small = Buffer::new("small\n");
    let small_uri = s.open_synced(&mut small, "small.txt");
    s.edit(&mut small, &small_uri, (0..0, "1"), 1);
    small
        .edit([(0..0, "now it's too long")], Origin::User)
        .unwrap();
    s.manager.changed(small.id());
    s.fake
        .wait_for_document(&small_uri, "didClose", |r| r.is_some_and(|r| !r.open));
    assert_eq!(s.manager.active_identity(small.id(), s.server), None);
    let reply = s.manager.document_request::<Echo>(
        small.id(),
        small.version(),
        |_| json!({ "small": true }),
    );
    assert_eq!(answer(reply).map(|_| ()), too_large());
    small.edit([(0..3, "")], Origin::User).unwrap();
    s.manager.changed(small.id());
    s.barrier();
    assert_eq!(s.methods(&small_uri), ["didOpen", "didChange", "didClose"]);
    assert_eq!(echoes(&s, "big") + echoes(&s, "small"), 0);

    small.edit([(0..14, "")], Origin::User).unwrap();
    s.manager.changed(small.id());
    let replica = s
        .fake
        .wait_for_document(&small_uri, "the reopen", |r| r.is_some_and(|r| r.open))
        .unwrap();
    assert_eq!((replica.version, replica.text.as_str()), (2, "1small\n"));
    assert!(s.manager.active_identity(small.id(), s.server).is_some());

    // An edit and then one that takes it over, drained together: the
    // didChange before the close carries the text before the paste.
    small.edit([(0..0, "2")], Origin::User).unwrap();
    small
        .edit([(0..0, "a paste far too long")], Origin::User)
        .unwrap();
    s.manager.changed(small.id());
    let replica = s
        .fake
        .wait_for_document(&small_uri, "didClose", |r| r.is_some_and(|r| !r.open))
        .unwrap();
    assert_eq!((replica.version, replica.text.as_str()), (3, "21small\n"));
    small.edit([(0..20, "")], Origin::User).unwrap();
    s.manager.changed(small.id());
    s.fake
        .wait_for_document(&small_uri, "the reopen", |r| r.is_some_and(|r| r.open));
    let reply = s.manager.document_request::<Echo>(
        small.id(),
        small.version(),
        |_| json!({ "small": true }),
    );
    let sent = answer(reply).unwrap().1.document.sent;
    assert_eq!(
        (sent.lsp_version, sent.text.to_string()),
        (4, "21small\n".into())
    );

    // A server that takes no open documents: requests still go by the
    // size of the latest text.
    let mut result = crate::fake::initialize_result(None);
    result.capabilities.text_document_sync = None;
    let mut s = Started::new(Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    });
    s.manager.set_max_file_bytes(16);
    let not_open = || unavailable("not open on a language server").map(|_| ());
    let ask = |s: &Started, buffer: &Buffer| {
        answer(s.manager.document_request::<Echo>(
            buffer.id(),
            buffer.version(),
            |_| json!({ "unsynced": true }),
        ))
        .map(|_| ())
    };
    let mut big = Buffer::new("seventeen bytes!\n");
    let big_uri = s.open(&mut big, "big.txt");
    assert_eq!(ask(&s, &big), too_large());
    let mut small = Buffer::new("small\n");
    let small_uri = s.open(&mut small, "small.txt");
    assert_eq!(ask(&s, &small), not_open());
    small
        .edit([(0..0, "now it's too long")], Origin::User)
        .unwrap();
    assert_eq!(ask(&s, &small), too_large());
    big.edit([(0..10, "")], Origin::User).unwrap();
    assert_eq!(ask(&s, &big), not_open());
    s.barrier();
    assert_eq!(echoes(&s, "unsynced"), 0);
    assert_eq!(s.methods(&big_uri), Vec::<String>::new());
    assert_eq!(s.methods(&small_uri), Vec::<String>::new());
}
