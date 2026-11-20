//! A plugin's `lsp.request` and `lsp.notify` with methods no table in fe-lsp lists, through the
//! real host to the fake server's session: fe-lsp sends any method (vendor/async-lsp's untyped
//! send), and only fe-plugin's `is_routed` judges which a plugin may.

use std::{
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_lsp::fake::{Record, Reply, Script, Scripted};
use fe_plugin::{
    HostState,
    fastedit::editor::lsp::{Host as _, Target},
    lsp::{LspRequests, State},
};
use gpui::TestAppContext;
use kiln_plugin::paths;
use serde_json::json;

use super::Bridge;
use super::CallDeadline;
use super::plugin_lsp_tests::{grants, on_plugin_thread};
use crate::plugin_bridge::serve;
use crate::tests::lsp::Lsp;

/// A request method no table in fe-lsp has ever listed.
const UNLISTED: &str = "acme/unlistedMethod";
/// A notification method no table in fe-lsp has ever listed.
const UNLISTED_NOTICE: &str = "acme/unlistedNotice";

/// A trusted window with `src/main.rs` open on the fake rust-analyzer, and a plugin's host state
/// reaching its sessions through the real bridge, kept with what it needs to stay up.
fn plugin_on_fake(cx: &mut TestAppContext) -> (Lsp, HostState, String, impl Sized) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    let requests = LspRequests::new();
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(30),
        PathBuf::new(),
        Arc::default(),
    );
    let plugin = HostState {
        lsp: State::new("acme", &grants(), Vec::new(), &requests, sessions),
        ..HostState::default()
    };
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    (s, plugin, root, (commands, requests))
}

#[gpui::test]
fn plugin_lsp_request_in_no_table_gets_the_fake_servers_reply(cx: &mut TestAppContext) {
    let (mut s, mut plugin, root, _keep) = plugin_on_fake(cx);
    s.fake.set_reply(
        UNLISTED,
        Scripted::now(Reply::Result(json!({ "unlisted": "answered" }))),
    );
    let answer = on_plugin_thread(&mut s, move || {
        plugin.request(
            "rust-analyzer".into(),
            Target::Root(root),
            UNLISTED.into(),
            "{\"asked\":1}".into(),
        )
    });
    let answer: serde_json::Value = serde_json::from_str(&answer.unwrap()).unwrap();
    assert_eq!(answer, json!({ "unlisted": "answered" }));
    let got = s.fake.records().into_iter().any(|record| {
        matches!(&record, Record::Request { method, params, .. }
            if method == UNLISTED && *params == json!({ "asked": 1 }))
    });
    assert!(got, "the fake server never saw {UNLISTED}");
}

#[gpui::test]
fn plugin_lsp_notify_in_no_table_reaches_the_fake_server(cx: &mut TestAppContext) {
    let (mut s, mut plugin, root, _keep) = plugin_on_fake(cx);
    let sent = on_plugin_thread(&mut s, move || {
        plugin.notify(
            "rust-analyzer".into(),
            Target::Root(root),
            UNLISTED_NOTICE.into(),
            "{\"told\":true}".into(),
        )
    });
    assert!(sent.is_ok(), "{sent:?}");
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.draw();
        let got = s.fake.records().into_iter().any(|record| {
            matches!(&record, Record::Notification { method, params }
                if method == UNLISTED_NOTICE && *params == json!({ "told": true }))
        });
        if got {
            break;
        }
        assert!(
            Instant::now() < until,
            "the fake server never got {UNLISTED_NOTICE}"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
