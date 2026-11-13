//! Owners (§5, §7 Owners): a colour's presentations, a link's resolve and
//! a hierarchy item's calls go to the server that produced the item alone,
//! never to another server, nor to a session the server restarted into.

use fe_buffer::Buffer;
use lsp_types::{
    CallHierarchyServerCapability, ColorProviderCapability, DocumentLinkOptions,
    ServerCapabilities, Url,
};
use serde_json::{Value, json};

use super::crash::{comes_back, crash};
use super::{Fixture, answer};
use crate::LspError;
use crate::fake::{Reply, Scripted};

const PRESENT: &str = "textDocument/colorPresentation";
const RESOLVE: &str = "documentLink/resolve";
const INCOMING: &str = "callHierarchy/incomingCalls";

/// Calls, for both servers.
fn calls(capabilities: &mut ServerCapabilities) {
    capabilities.call_hierarchy_provider = Some(CallHierarchyServerCapability::Simple(true));
}

/// Calls, colours and links it resolves: the add-on's.
fn everything(capabilities: &mut ServerCapabilities) {
    calls(capabilities);
    capabilities.color_provider = Some(ColorProviderCapability::Simple(true));
    capabilities.document_link_provider = Some(DocumentLinkOptions {
        resolve_provider: Some(true),
        work_done_progress_options: Default::default(),
    });
}

/// The first `len` characters of row 0.
fn head(len: u32) -> Value {
    json!({
        "start": { "line": 0, "character": 0 },
        "end": { "line": 0, "character": len },
    })
}

/// A function named `name` in `uri`, as a hierarchy item.
fn item(name: &str, uri: &Url) -> Value {
    json!({
        "name": name,
        "kind": 12,
        "uri": uri,
        "range": head(1),
        "selectionRange": head(1),
    })
}

/// Whether `answer` says the item's server restarted.
fn restarted<T>(answer: &Result<T, LspError>) -> bool {
    matches!(answer, Err(LspError::Unavailable(why)) if *why == "the language server restarted")
}

#[test]
fn follow_ups_reach_only_the_producing_server_never_a_restarted_session() {
    // The primary has calls, but neither colours nor links, and no call
    // hierarchy at the start of the row: everything comes from the add-on.
    let mut f = Fixture::css_and_vars_with(calls, everything);
    let mut buffer = Buffer::new("red {}\n");
    let uri = f.open_on_both(&mut buffer);
    let vars = f.placed(&buffer, "vars").unwrap();
    let reply = |name: &str, method: &str, result: Value| {
        f.fake(name)
            .set_reply(method, Scripted::now(Reply::Result(result)));
    };
    reply(
        "vars",
        "textDocument/documentColor",
        json!([{ "range": head(3), "color": { "red": 1.0, "green": 0.0, "blue": 0.0, "alpha": 1.0 } }]),
    );
    reply(
        "vars",
        "textDocument/documentLink",
        json!([{ "range": head(3), "data": 7 }]),
    );
    reply("css", "textDocument/prepareCallHierarchy", json!([]));
    reply(
        "vars",
        "textDocument/prepareCallHierarchy",
        json!([item("red", &uri)]),
    );
    reply("vars", PRESENT, json!([{ "label": "#ff0000" }]));
    reply(
        "vars",
        RESOLVE,
        json!({ "range": head(3), "target": "https://red.example/" }),
    );
    reply(
        "vars",
        INCOMING,
        json!([{ "from": item("caller", &uri), "fromRanges": [] }]),
    );

    let (id, version) = (buffer.id(), buffer.version());
    let colors = answer(f.manager.document_colors(id, version))
        .unwrap()
        .unwrap();
    let links = answer(f.manager.document_links(id, version))
        .unwrap()
        .unwrap();
    let items = answer(f.manager.prepare_call_hierarchy(id, version, 0))
        .unwrap()
        .unwrap();
    let (color, link, item) = (colors[0].clone(), links[0].clone(), items[0].clone());
    for owner in [color.owner(), link.owner(), item.owner()] {
        assert_eq!(owner.map(|owner| owner.server), Some(vars));
    }

    // Each follow-up reaches the add-on, and only it.
    let presentations = answer(f.manager.color_presentations(id, version, color.clone()));
    assert_eq!(presentations.unwrap()[0].label, "#ff0000");
    let resolved = answer(f.manager.resolve_document_link(id, version, link.clone()));
    assert_eq!(
        resolved.unwrap().target.as_deref(),
        Some("https://red.example/")
    );
    let edges = answer(f.manager.incoming_calls(id, &item)).unwrap();
    assert_eq!(edges[0].item.name, "caller");
    assert_eq!(
        edges[0].item.owner(),
        item.owner(),
        "a result keeps the owner"
    );
    for method in [PRESENT, RESOLVE, INCOMING] {
        assert_eq!(f.asked("vars", method).len(), 1, "{method}");
        assert!(
            f.asked("css", method).is_empty(),
            "the primary got {method}"
        );
    }

    // The add-on crashes and comes back: its new session has the document,
    // but none of the items, and is never asked about them.
    crash(&f, "vars", vars);
    comes_back(&f, "vars", vars, &uri, &buffer);
    let presentations = answer(f.manager.color_presentations(id, version, color));
    assert!(restarted(&presentations), "{presentations:?}");
    let resolved = answer(f.manager.resolve_document_link(id, version, link));
    assert!(restarted(&resolved), "{resolved:?}");
    let edges = answer(f.manager.incoming_calls(id, &item));
    assert!(
        matches!(&edges, Err(LspError::Unavailable(why)) if *why == "stale"),
        "{edges:?}"
    );
    f.barrier(vars);
    f.barrier(f.placed(&buffer, "css").unwrap());
    for method in [PRESENT, RESOLVE, INCOMING] {
        assert!(
            f.fake("vars")
                .records_of(2)
                .iter()
                .all(|r| r.method() != method),
            "the restarted add-on got {method}"
        );
        assert!(
            f.asked("css", method).is_empty(),
            "the primary got {method}"
        );
    }
}
