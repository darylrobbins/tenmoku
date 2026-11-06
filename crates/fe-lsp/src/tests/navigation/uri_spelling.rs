//! A server that escapes `[ ] ( )` in URIs (vscode-uri, typescript-go) names
//! the same file as we do with other bytes: files are matched by decoded
//! path, so the edited buffer is still what a location is decoded against.

use std::sync::atomic::AtomicBool;

use fe_buffer::{Buffer, Origin};
use fe_nav::{NavRequest, Tier};
use lsp_types::Url;
use serde_json::json;

use super::{Nav, location, open, range};
use crate::ReadinessMode;

/// On disk; the buffers differ.
const ON_DISK: &str = "// disk\nfn slug() {}\n";
const BUFFER: &str = "fn slug() {}\n";

fn respelled(uri: &Url, from: &str, to: &str) -> Url {
    let spelled = uri.as_str().replace(from, to);
    assert_ne!(spelled, uri.as_str());
    Url::parse(&spelled).unwrap()
}

#[test]
fn definition_in_own_file_with_respelled_uri_decodes_against_the_buffer() {
    let s = Nav::ready(ReadinessMode::ServerStatus);
    for dir in ["app/[slug]", "app/(group)"] {
        std::fs::create_dir_all(s.root.join(dir)).unwrap();
    }
    let (page, other) = ("app/[slug]/page.rs", "app/(group)/other.rs");
    for rel in [page, other] {
        std::fs::write(s.root.join(rel), ON_DISK).unwrap();
    }

    let mut page_buffer = Buffer::new(BUFFER);
    let mut other_buffer = Buffer::new(BUFFER);
    for (buffer, rel) in [(&mut page_buffer, page), (&mut other_buffer, other)] {
        open(&mut s.manager.lock().unwrap(), &s.root, buffer, rel);
        s.fake
            .wait_for_document(&s.uri(rel), "didOpen", |r| r.is_some());
    }
    let other_sent = other_buffer.version();
    other_buffer
        .edit([(0..0, "// edited\n")], Origin::User)
        .unwrap();

    let req = NavRequest {
        rel: page.into(),
        text: page_buffer.text().clone(),
        offset: 3,
        buffer: Some((page_buffer.id(), page_buffer.version())),
        ..s.request()
    };
    let page_uri = respelled(&s.uri(page), "[slug]", "%5Bslug%5D");
    let other_uri = respelled(&s.uri(other), "(group)", "%28group%29");
    assert_ne!(page_uri, s.uri(page));

    // `slug` in the buffer is at 3..7; in the file on disk it is not.
    s.reply(
        "textDocument/definition",
        json!([location(&page_uri, range(0, 3, 7))]),
    );
    let answer = s.definitions(&req);
    assert_eq!(answer.value.items.len(), 1);
    let item = &answer.value.items[0];
    assert_eq!(item.tier, Tier::SameFile);
    assert_eq!(item.location.snapshot, req.buffer);
    assert_eq!(item.location.rel.as_path(), std::path::Path::new(page));
    assert_eq!(item.location.range, 3..7);
    assert_eq!(item.location.row, 0);

    // Another open, edited file: the text the server had, not the disk's.
    s.reply(
        "textDocument/definition",
        json!([location(&other_uri, range(0, 3, 7))]),
    );
    let answer = s.definitions(&req);
    assert_eq!(answer.value.items.len(), 1);
    let item = &answer.value.items[0];
    assert_eq!(item.location.rel.as_path(), std::path::Path::new(other));
    assert_eq!(item.location.range, 3..7);
    assert_eq!(item.location.row, 0);
    assert_eq!(
        item.location.snapshot,
        Some((other_buffer.id(), other_sent))
    );

    // Nothing on disk: the own file is still the buffer.
    std::fs::remove_dir_all(s.root.join("app")).unwrap();
    s.reply(
        "textDocument/definition",
        json!([location(&page_uri, range(0, 3, 7))]),
    );
    let cancel = AtomicBool::new(false);
    let answer = s.navigation.definitions(&req, &cancel).unwrap();
    assert_eq!(answer.value.items[0].tier, Tier::SameFile);
    assert_eq!(answer.value.items[0].location.range, 3..7);
}
