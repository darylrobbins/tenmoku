//! Workspace symbols across mixed languages (§5, §7 Merging): every
//! session ready at the root is asked once, whatever its languages, add-ons
//! included, and a symbol two servers report is listed once.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_nav::{NavigationProvider, Readiness};
use serde_json::{Value, json};

use super::{CSS, Fixture, ROOT, spec};
use crate::fake::{Reply, Scripted};
use crate::{LspNavigation, ServerStatus};

const SYMBOL: &str = "workspace/symbol";
const TS: &str = "x-typescript";
const RUST: &str = "x-rust";

/// A symbol named `name`, of `kind`, on `uri`'s first row.
fn symbol(name: &str, kind: u32, uri: &lsp_types::Url) -> Value {
    json!({
        "name": name,
        "kind": kind,
        "location": {
            "uri": uri,
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 4 },
            },
        },
    })
}

#[test]
fn mixed_languages_each_server_asked_once_and_duplicates_listed_once() {
    // A TypeScript primary, an add-on for TypeScript and CSS, and a Rust
    // server, all at one root.
    let mut f = Fixture::new(&["ts", "vars", "ra"]);
    f.manager.register_spec(spec("ts", &[TS], false));
    f.manager.register_spec(spec("vars", &[TS, CSS], true));
    f.manager.register_spec(spec("ra", &[RUST], false));
    // A CSS primary that no document has started.
    f.manager.register_spec(spec("css", &[CSS], false));
    let mut script = Buffer::new("elem;\n");
    let ts_uri = f.open(&mut script, "a.ts", TS);
    let mut source = Buffer::new("fn elem() {}\n");
    let rs_uri = f.open(&mut source, "main.rs", RUST);
    f.wait_synced("ts", &ts_uri, &script);
    f.wait_synced("vars", &ts_uri, &script);
    f.wait_synced("ra", &rs_uri, &source);

    // Each is ready once its quiet period has passed.
    let deadline = Instant::now() + Duration::from_secs(10);
    let ready = |f: &Fixture| {
        (["ts", "vars", "ra"].iter())
            .all(|name| f.manager.status_at(name, Path::new(ROOT)) == Some(ServerStatus::Ready))
    };
    while !ready(&f) {
        assert!(Instant::now() < deadline, "the servers never became ready");
        f.clock.advance(crate::readiness::SETTLE);
        std::thread::sleep(Duration::from_millis(5));
    }

    // The add-on reports the primary's `elemShared` too, as it is.
    let replies = [
        (
            "ts",
            json!([
                symbol("elemTs", 13, &ts_uri),
                symbol("elemShared", 13, &ts_uri)
            ]),
        ),
        (
            "vars",
            json!([
                symbol("elemShared", 13, &ts_uri),
                symbol("elemVar", 13, &ts_uri)
            ]),
        ),
        ("ra", json!([symbol("elemRust", 12, &rs_uri)])),
    ];
    for (name, reply) in replies {
        f.fake(name)
            .set_reply(SYMBOL, Scripted::now(Reply::Result(reply)));
    }
    let (idle, _) = super::super::manager(f.fake("ts").launcher(), None);
    let manager = Arc::new(Mutex::new(std::mem::replace(&mut f.manager, idle)));
    let lsp = LspNavigation::new(Arc::downgrade(&manager), ROOT);
    // CSS is ready through its add-on, though its primary isn't started.
    assert_eq!(lsp.readiness(Some(CSS)), Readiness::Ready);
    assert_eq!(lsp.readiness(Some("x-none")), Readiness::Unsupported);
    let found = lsp
        .workspace_symbols("elem", 10, &AtomicBool::new(false))
        .unwrap();

    let mut names: Vec<_> = found.iter().map(|symbol| symbol.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["elemRust", "elemShared", "elemTs", "elemVar"]);
    for name in ["ts", "vars", "ra"] {
        assert_eq!(f.asked(name, SYMBOL).len(), 1, "{name} asked once");
    }
}
