//! LSP glob patterns, and notifications to a manager's servers.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use lsp_types::{GlobPattern, OneOf, RelativePattern, Url};
use serde_json::json;

use super::WATCHED;
use crate::fake::{FakeServer, Record, Reply, Script, Scripted};
use crate::glob::LspGlob;
use crate::tests::{ROOT, Started, answer, manager};
use crate::{ManualClock, ServerSpec};

fn string(pattern: &str) -> LspGlob {
    LspGlob::new(&GlobPattern::String(pattern.to_string())).unwrap()
}

fn relative(base: &str, pattern: &str) -> Result<LspGlob, String> {
    let pattern = GlobPattern::Relative(RelativePattern {
        base_uri: OneOf::Right(Url::parse(base).unwrap()),
        pattern: pattern.into(),
    });
    LspGlob::new(&pattern)
}

#[track_caller]
fn check(glob: &LspGlob, matching: &[&str], not: &[&str]) {
    for path in matching {
        assert!(glob.matches(Path::new(path)), "should match {path}");
    }
    for path in not {
        assert!(!glob.matches(Path::new(path)), "shouldn't match {path}");
    }
}

#[test]
fn lsp_globs_match_as_vs_code() {
    check(
        &string("**/*.rs"),
        &["/r/a.rs", "/r/src/b/c.rs"],
        &["/r/a.rsx"],
    );
    check(
        &string("**/{Cargo.toml,Cargo.lock}"),
        &["/r/Cargo.toml", "/r/Cargo.lock", "/r/a/b/Cargo.toml"],
        &["/r/Cargo.json"],
    );
    check(
        &string("**/src/?.rs"),
        &["/r/src/a.rs"],
        &["/r/src/ab.rs", "/r/src/a/b.rs"],
    );
    check(&string("**/[!.]*.py"), &["/r/x.py"], &["/r/.x.py"]);
    check(&string("**/[a-c].py"), &["/r/b.py"], &["/r/d.py"]);
    check(
        &string("**/node_modules/**"),
        &["/r/a/node_modules/b/c.js"],
        &["/r/a/modules/b.js"],
    );
    // A trailing `/**` matches the directory itself, as in VS Code.
    check(
        &string("**/node_modules/**"),
        &["/r/node_modules", "/r/a/node_modules"],
        &[],
    );
    check(&string("/r/{a/**,b}"), &["/r/a", "/r/a/x", "/r/b"], &[]);
    // An empty alternative is allowed.
    check(
        &string("**/foo{,.rs}"),
        &["/r/foo", "/r/foo.rs"],
        &["/r/foo.py"],
    );
    // A backslash is an ordinary character; `*` after it stays a wildcard.
    check(&string(r"**/foo\*.rs"), &[r"/r/foo\x.rs"], &["/r/foo*.rs"]);
    // A known limit: globset matches bytes, so `?` is one byte of a
    // character, and doesn't match `é` (two).
    check(&string("**/src/?.rs"), &["/r/src/a.rs"], &["/r/src/é.rs"]);
    // A string pattern is matched against the whole absolute path.
    check(&string("*.toml"), &[], &["/r/Cargo.toml", "/Cargo.toml"]);
}

#[test]
fn relative_patterns_match_under_their_base() {
    let glob = relative("file:///r/sub", "*.go").unwrap();
    check(
        &glob,
        &["/r/sub/a.go"],
        &["/r/a.go", "/r/sub/x/a.go", "/r/subs/a.go"],
    );
    let glob = relative("file:///r/sub", "foo{,.go}").unwrap();
    check(&glob, &["/r/sub/foo", "/r/sub/foo.go"], &["/r/foo"]);
    let glob = relative("file:///r/sub", "gen/**").unwrap();
    check(&glob, &["/r/sub/gen", "/r/sub/gen/a/b.go"], &["/r/gen"]);
    assert!(relative("file:///r/sub", "[").is_err());
    assert!(LspGlob::new(&GlobPattern::String("[".into())).is_err());
}

#[test]
fn notifications_reach_the_session_in_order() {
    // Before the fake has initialized, nothing is sent.
    let fake_clock = Arc::new(ManualClock::new());
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: Duration::from_secs(1),
            ..Script::default()
        },
        fake_clock.clone(),
    );
    let (mut early, _events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    early
        .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
        .unwrap();
    fake.wait_for("initialize", |records| {
        records.iter().any(|record| record.method() == "initialize")
    });
    early.notify_sessions(WATCHED, |_, _| Some(json!({ "changes": [] })));
    fake_clock.wait_for_sleep(Duration::from_secs(1));
    fake_clock.advance(Duration::from_secs(1));
    fake.wait_initialized();
    assert!(
        !fake
            .records()
            .iter()
            .any(|record| record.is_notification(WATCHED))
    );

    let mut s = Started::new(Script::default());
    s.fake.set_reply(
        "textDocument/hover",
        Scripted::now(Reply::Result(json!({ "contents": "main" }))),
    );
    let mut buffer = Buffer::new("fn main() {}\n");
    s.open_synced(&mut buffer, "a.rs");
    // Queued before the notification, so it arrives before it.
    let hover = s.manager.hover(buffer.id(), buffer.version(), 1);
    let server = s.server;
    let params = json!({ "changes": [{ "uri": "file:///fake/root/a.rs", "type": 2 }] });
    s.manager.notify_sessions(WATCHED, |id, _| {
        assert_eq!(id, server);
        Some(params.clone())
    });
    answer(hover).unwrap();
    s.fake.wait_for("the notification", |records| {
        records.iter().any(|record| record.is_notification(WATCHED))
    });
    let records = s.fake.records();
    let at = |wanted: &dyn Fn(&Record) -> bool| records.iter().position(wanted).unwrap();
    assert!(
        at(&|r| r.method() == "textDocument/hover") < at(&|r| r.is_notification(WATCHED)),
        "{records:?}"
    );
    assert!(records.contains(&Record::Notification {
        method: WATCHED.into(),
        params: params.clone(),
    }));

    // None sends nothing: a marker sent after it is the next notification.
    s.manager.notify_sessions(WATCHED, |_, _| None);
    s.manager
        .notify_sessions("workspace/didCreateFiles", |_, _| {
            Some(json!({ "files": [] }))
        });
    s.fake.wait_for("the marker", |records| {
        records
            .iter()
            .any(|record| record.is_notification("workspace/didCreateFiles"))
    });
    let sent = (s.fake.records().into_iter())
        .filter(|record| record.is_notification(WATCHED))
        .count();
    assert_eq!(sent, 1);
}
