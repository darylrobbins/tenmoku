//! A plugin's `lsp.request` and `lsp.notify` through the real host: the bridge to the main
//! thread, the window's fe-lsp manager and a fake server's session. The plugin's imports are
//! called on a thread of the test's own, as its thread calls them, while the main thread takes
//! the bridge's jobs.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use fe_command::capability::{Capability, Grants};
use fe_lsp::fake::{Record, Reply, Script, Scripted};
use fe_plugin::{
    HostState,
    fastedit::editor::lsp::{Host as _, Target},
    lsp::{LspRequests, State},
};
use gpui::TestAppContext;
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln_plugin::{bindings::Error, paths};
use kiln_plugin::{
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
    runner::{Call, Reply as PluginReply},
};
use serde_json::json;

use super::{Bridge, CallDeadline, open_editors, runs_for};
use crate::plugin_bridge::serve;
use crate::{plugin_host::Found, tests::lsp::Lsp};

const PLUGIN: &str = "acme";
const METHOD: &str = "fastedit-test/custom";
const NOTICE: &str = "fastedit-test/notice";

pub(super) fn grants() -> Grants {
    ["lsp:rust-analyzer", "buffer.read", "fs.read:workspace"]
        .into_iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// Runs `call` on a thread of its own, as a plugin's thread calls, and returns what it did, the
/// main thread taking jobs meanwhile.
pub(super) fn on_plugin_thread<R: Send + 'static>(
    s: &mut Lsp,
    call: impl FnOnce() -> R + Send + 'static,
) -> R {
    let thread = thread::spawn(call);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !thread.is_finished() {
        assert!(Instant::now() < deadline, "the call didn't return");
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    thread.join().unwrap()
}

#[gpui::test]
fn a_plugins_lsp_request_reaches_the_fake_servers_session(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: Reply::Result(json!({"answered": true})),
            delay: Duration::ZERO,
        },
    );
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
    let mut plugin = HostState {
        lsp: State::new(PLUGIN, &grants(), Vec::new(), &requests, sessions),
        ..HostState::default()
    };

    // By the root.
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let answer = on_plugin_thread(&mut s, move || {
        let target = Target::Root(root);
        let answer = plugin.request(
            "rust-analyzer".into(),
            target,
            METHOD.into(),
            "{\"x\":1}".into(),
        );
        (answer, plugin)
    });
    let (answer, mut plugin) = answer;
    let answer: serde_json::Value = serde_json::from_str(&answer.unwrap()).unwrap();
    assert_eq!(answer, json!({"answered": true}));
    let got = s.fake.records().into_iter().any(|record| {
        matches!(&record, Record::Request { method, params, .. }
            if method == METHOD && *params == json!({"x": 1}))
    });
    assert!(got, "the fake server never saw {METHOD}");

    // By the open buffer, which is placed on that server.
    // Looked up on the main thread, as the plugin's own lookups run, not inside a window update.
    let asker = bridge.clone();
    let buffer = on_plugin_thread(&mut s, move || {
        let until = Instant::now() + Duration::from_secs(30);
        asker.on_main(until, |_, cx| open_editors(cx).first().map(|(id, ..)| *id))
    })
    .unwrap()
    .expect("an open buffer");
    let (answer, mut plugin) = on_plugin_thread(&mut s, move || {
        let answer = plugin.request(
            "rust-analyzer".into(),
            Target::Buffer(buffer),
            METHOD.into(),
            "null".into(),
        );
        (answer, plugin)
    });
    assert_eq!(answer.unwrap(), "{\"answered\":true}");

    // A root no window has, and a method the editor routes itself, are refused.
    let (missing, mut plugin) = on_plugin_thread(&mut s, move || {
        let missing = plugin.request(
            "rust-analyzer".into(),
            Target::Root("/no/such/root".into()),
            METHOD.into(),
            "null".into(),
        );
        (missing, plugin)
    });
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");
    let routed = on_plugin_thread(&mut s, move || {
        plugin.request(
            "rust-analyzer".into(),
            Target::Buffer(buffer),
            "textDocument/hover".into(),
            "null".into(),
        )
    });
    assert!(matches!(routed, Err(Error::Denied(_))), "{routed:?}");
}

#[test]
fn only_the_running_contribution_of_a_server_name_is_the_extensions_own() {
    let (a, b) = (Path::new("/ext/a"), Path::new("/ext/b"));
    // A's `tls` is disabled or unconsented, so it isn't handed to the managers; B's runs.
    let running = [(b, "tls")];
    assert!(!runs_for(running, "tls", a));
    assert!(runs_for(running, "tls", b));
    // Another name of its own, or nothing running at all.
    assert!(!runs_for(running, "lint", b));
    assert!(!runs_for([], "tls", a));
}

#[gpui::test]
fn an_extension_with_no_running_server_of_that_name_is_denied_both_imports(
    cx: &mut TestAppContext,
) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    // The extension declares `tls` and holds its grant, but no consent makes it run.
    let granted = ["lsp:tls", "fs.read:workspace"]
        .into_iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect();
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(30),
        "/ext/loser".into(),
        Arc::default(),
    );
    let mut plugin = HostState {
        lsp: State::new(
            "loser",
            &granted,
            vec!["tls".into()],
            &LspRequests::new(),
            sessions,
        ),
        ..HostState::default()
    };
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let (request, notify) = on_plugin_thread(&mut s, move || {
        let target = || Target::Root(root.clone());
        let request = plugin.request("tls".into(), target(), METHOD.into(), "null".into());
        let notify = plugin.notify("tls".into(), target(), METHOD.into(), "null".into());
        (request, notify)
    });
    assert!(matches!(request, Err(Error::Denied(_))), "{request:?}");
    assert!(matches!(notify, Err(Error::Denied(_))), "{notify:?}");
}

/// A plugin whose `run-command` sends `fastedit-test/custom` to rust-analyzer's session at the
/// root it is given as its args, and returns ok whatever came back.
const LSP_CALLER: &str = r#"
(component $C
  (import "kiln:extension/types@1.0.0" (instance $types
    (type $range (record (field "start" u64) (field "end" u64)))
    (export "range" (type (eq $range)))
    (type $error (variant
      (case "unimplemented" string)
      (case "denied" string)
      (case "not-found" string)
      (case "invalid-argument" string)
      (case "conflict" string)
      (case "failed" string)))
    (export "error" (type (eq $error)))))
  (alias export $types "range" (type $range))
  (alias export $types "error" (type $error))

  (import "fastedit:editor/lsp@1.1.0" (instance $lsp
    (alias outer $C $error (type $error))
    (type $target (variant (case "buffer" u64) (case "root" string)))
    (export "target" (type $target' (eq $target)))
    (export "request" (func
      (param "server" string) (param "target" $target') (param "method" string) (param "params" string)
      (result (result string (error $error)))))))

  (import "kiln:extension/events@1.0.0" (instance $events
    (alias outer $C $range (type $range))
    (type $buffer-opened (record
      (field "buffer" u64)
      (field "version" u64)
      (field "path" (option string))
      (field "language" (option string))))
    (export "buffer-opened" (type $buffer-opened' (eq $buffer-opened)))
    (type $buffer-changed (record
      (field "buffer" u64) (field "version" u64) (field "ranges" (list $range))))
    (export "buffer-changed" (type $buffer-changed' (eq $buffer-changed)))
    (type $buffer-saved (record
      (field "buffer" u64) (field "version" u64) (field "path" string)))
    (export "buffer-saved" (type $buffer-saved' (eq $buffer-saved)))
    (type $selection-changed (record
      (field "buffer" u64) (field "version" u64) (field "ranges" (list $range))))
    (export "selection-changed" (type $selection-changed' (eq $selection-changed)))
    (type $event (variant
      (case "buffer-opened" $buffer-opened')
      (case "buffer-changed" $buffer-changed')
      (case "buffer-saved" $buffer-saved')
      (case "buffer-closed" u64)
      (case "focus-changed" (option u64))
      (case "selection-changed" $selection-changed')
      (case "config-reloaded")
      (case "lagged" u64)))
    (export "event" (type (eq $event)))))
  (alias export $events "event" (type $event))

  (core module $libc
    (memory (export "memory") 1)
    ;; Bytes 0..8 stay zero: a `result` that is `ok`.
    (data (i32.const 256) "rust-analyzer")
    (data (i32.const 288) "fastedit-test/custom")
    (data (i32.const 320) "null")
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
      (local $p i32)
      (local.set $p
        (i32.and
          (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (global.set $heap (i32.add (local.get $p) (local.get $size)))
      (local.get $p)))
  (core instance $libc (instantiate $libc))
  (alias core export $libc "memory" (core memory $memory))
  (alias core export $libc "realloc" (core func $realloc))

  (alias export $lsp "request" (func $lsp-request))
  (core func $request (canon lower (func $lsp-request) (memory $memory) (realloc $realloc)))
  (core instance $host (export "request" (func $request)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "request" (func $request
      (param i32 i32 i32 i64 i32 i32 i32 i32 i32 i32)))
    (func (export "init") (result i32) (i32.const 0))
    (func (export "shutdown"))
    (func (export "run-command") (param $id i32) (param $id-len i32) (param $args i32) (param $args-len i32) (result i32)
      ;; server "rust-analyzer", the root variant (case 1) holding the args, method, params "null".
      (call $request
        (i32.const 256) (i32.const 13)
        (i32.const 1) (i64.extend_i32_u (local.get $args)) (local.get $args-len)
        (i32.const 288) (i32.const 20)
        (i32.const 320) (i32.const 4)
        (i32.const 512))
      (i32.const 0))
    (func (export "handle-events") (param i32 i32)))
  (core instance $main (instantiate $main
    (with "libc" (instance $libc))
    (with "host" (instance $host))))

  (func (export "init") (result (result (error $error)))
    (canon lift (core func $main "init") (memory $memory)))
  (func (export "shutdown")
    (canon lift (core func $main "shutdown")))

  (func $run-command (param "id" string) (param "args" string) (result (result (error $error)))
    (canon lift (core func $main "run-command") (memory $memory) (realloc $realloc)))
  (instance $command-handler (export "run-command" (func $run-command)))
  (export "kiln:extension/command-handler@1.0.0" (instance $command-handler))

  (func $handle-events (param "events" (list $event))
    (canon lift (core func $main "handle-events") (memory $memory) (realloc $realloc)))
  (instance $event-handler (export "handle-events" (func $handle-events)))
  (export "kiln:extension/event-handler@1.0.0" (instance $event-handler)))
"#;

/// Installs `LSP_CALLER` as plugin `id`, granted its `lsp:rust-analyzer` and an fs capability.
fn install(dir: &Path, id: &str, wat: &str) -> Found {
    let ext = dir.join("extensions").join(id);
    std::fs::create_dir_all(&ext).unwrap();
    std::fs::write(ext.join("plugin.wasm"), wat.as_bytes()).unwrap();
    let capabilities = ["lsp:rust-analyzer", "fs.read:workspace"];
    let quoted: Vec<String> = capabilities.iter().map(|c| format!("{c:?}")).collect();
    std::fs::write(
        ext.join("extension.toml"),
        format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [{}]\n",
            quoted.join(", ")
        ),
    )
    .unwrap();
    let grants: Grants = (capabilities.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect();
    GrantStore::new(dir.join("state").join(crate::plugin_host::GRANTS_FILE))
        .grant(id, (1, 0, 0), &grants, &[], false)
        .unwrap();
    let api = ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![crate::plugin_host::plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = load_manifest(&ext, &api);
    let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    Found::new(&manifest, Ok(plugin))
}

/// Unloading a loaded plugin through the host, while its call waits on a slow server, drops the
/// request at once: the call holds the plugin's mutex, which the unload and its shutdown wait for.
#[gpui::test]
fn unloading_a_plugin_through_the_host_drops_its_waiting_request(cx: &mut TestAppContext) {
    use crate::plugin_host::{
        PluginHost, PluginState, plugin_handle, plugin_states, set_plugin_dirs, set_plugins,
        set_preview, unload_plugin,
    };
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    // Plugins run on threads of their own.
    s.cx.executor().allow_parking();
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: Reply::Result(json!(null)),
            delay: Duration::from_secs(600),
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let (state, cache) = (dir.path().join("state"), dir.path().join("cache"));
    s.cx.update(|_, cx| {
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(60);
    });
    let found = install(dir.path(), "acme.lspcall", LSP_CALLER);
    s.cx.update(|_, cx| set_plugins(cx, vec![found]));
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.draw();
        let running = s.cx.update(|_, cx| {
            (plugin_states(cx).iter())
                .any(|info| info.id == "acme.lspcall" && info.state == PluginState::Running)
        });
        if running {
            break;
        }
        assert!(Instant::now() < until, "the plugin didn't load");
        thread::sleep(Duration::from_millis(5));
    }

    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let handle =
        s.cx.update(|_, cx| plugin_handle("acme.lspcall", cx))
            .unwrap();
    let call = thread::spawn(move || {
        handle.lock().unwrap().call(Call::RunCommand {
            id: "acme.lspcall.go".into(),
            args: root,
        })
    });
    // The fake server has the request, which it holds for ten minutes.
    while !s
        .fake
        .records()
        .iter()
        .any(|record| matches!(record, Record::Request { method, .. } if method == METHOD))
    {
        assert!(Instant::now() < until, "the server never got the request");
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    let started = Instant::now();
    s.cx.update(|_, cx| unload_plugin("acme.lspcall", cx));
    while !call.is_finished() {
        assert!(
            Instant::now() < until,
            "the unloaded plugin's call kept waiting"
        );
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the call outlived the unload by {:?}",
        started.elapsed()
    );
    // It ends one way or another; what it returned doesn't matter.
    let _ = call
        .join()
        .unwrap()
        .map(|reply| matches!(reply, PluginReply::RunCommand(_)));
    assert_eq!(
        crate::plugin_host::lsp_requests().pending("acme.lspcall"),
        0
    );
}

/// A call whose plugin unloads before the main thread takes its job never reaches the server.
#[gpui::test]
fn a_cancelled_call_is_not_sent_after_the_unload(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    // No `serve` yet: the job waits in the bridge's queue.
    let (bridge, jobs) = Bridge::new();
    let requests = LspRequests::new();
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(60),
        PathBuf::new(),
        Arc::default(),
    );
    let mut plugin = HostState {
        lsp: State::new("late", &grants(), Vec::new(), &requests, sessions),
        ..HostState::default()
    };
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let thread = thread::spawn(move || {
        plugin.notify(
            "rust-analyzer".into(),
            Target::Root(root),
            "fastedit-test/notice".into(),
            "null".into(),
        )
    });
    let until = Instant::now() + Duration::from_secs(30);
    while requests.pending("late") == 0 {
        assert!(Instant::now() < until, "the call never waited");
        thread::sleep(Duration::from_millis(2));
    }
    requests.unload("late");
    let result = thread.join().unwrap();
    assert!(matches!(result, Err(Error::Denied(_))), "{result:?}");
    // The queued job runs now, and finds its plugin gone.
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    for _ in 0..50 {
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    let sent = s.fake.records().iter().any(|record| {
        matches!(record, Record::Notification { method, .. } if method == "fastedit-test/notice")
    });
    assert!(!sent, "the notification was sent after the plugin unloaded");
}

/// A consented, non-built-in server `taplo` of an extension in `dir`.
fn taplo(dir: &Path) -> crate::ExtensionServer {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("server.js"), "console.log(1)\n").unwrap();
    crate::ExtensionServer {
        extension_id: "acme.toml".into(),
        extension_name: "Toml".into(),
        extension_version: "1.0.0".into(),
        extension_license: "MIT".into(),
        extension_dir: dunce::canonicalize(dir).unwrap(),
        spec: fe_lsp::ServerSpec {
            args: vec!["server.js".into()],
            languages: vec!["toml".into()],
            ..fe_lsp::ServerSpec::new("taplo", "/opt/taplo/bin/taplo")
        },
    }
}

/// A plugin of the extension in `extension`, declaring `taplo` and granted it.
fn taplo_plugin(
    bridge: &Bridge,
    extension: &Path,
    requests: &LspRequests,
    name: &str,
) -> HostState {
    let granted = ["lsp:taplo", "fs.read:workspace"]
        .into_iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect();
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(30),
        extension.to_owned(),
        Arc::default(),
    );
    HostState {
        lsp: State::new(name, &granted, vec!["taplo".into()], requests, sessions),
        ..HostState::default()
    }
}

/// A plugin reaches the consented, non-built-in server its own extension contributes, by request
/// and notification, before and after the server's invocation changes and awaits consent (the
/// granted one keeps running), and another extension declaring the same name stays denied.
#[gpui::test]
fn a_plugin_reaches_its_extensions_consented_server_through_a_pending_change(
    cx: &mut TestAppContext,
) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    let dir = tempfile::tempdir().unwrap();
    let extension_dir = dir.path().join("toml");
    let granted = taplo(&extension_dir);
    std::fs::write(s.path("a.toml"), "a = 1\n").unwrap();
    s.cx.update(|_, cx| crate::set_extension_servers(cx, vec![granted.clone()]));
    s.draw();
    let asked = s.cx.update(|_, cx| crate::pending_extension_consents(cx));
    let asked = asked.iter().find(|p| p.server == "taplo").expect("asked");
    s.cx.update(|_, cx| crate::grant_extension_server(cx, asked))
        .unwrap();
    s.draw();
    s.open("a.toml");
    s.until("taplo to launch", |s| s.fake.launches() >= 1);
    let main = s.uri("a.toml");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: Reply::Result(json!({"from": "taplo"})),
            delay: Duration::ZERO,
        },
    );

    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    let requests = LspRequests::new();
    let canonical = dunce::canonicalize(&extension_dir).unwrap();
    let owner = taplo_plugin(&bridge, &canonical, &requests, "acme.toml");
    let other_dir = dir.path().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other = taplo_plugin(&bridge, &other_dir, &requests, "acme.other");
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");

    // Both imports, from the owner and from another extension.
    let round = |s: &mut Lsp, owner: HostState, other: HostState| {
        let root = root.clone();
        on_plugin_thread(s, move || {
            let (mut owner, mut other) = (owner, other);
            let to = || ("taplo".to_owned(), Target::Root(root.clone()));
            let ((s1, t1), (s2, t2), (s3, t3), (s4, t4)) = (to(), to(), to(), to());
            let a = owner.request(s1, t1, METHOD.into(), "null".into());
            let b = owner.notify(s2, t2, NOTICE.into(), "null".into());
            let c = other.request(s3, t3, METHOD.into(), "null".into());
            let d = other.notify(s4, t4, NOTICE.into(), "null".into());
            (a, b, c, d, owner, other)
        })
    };
    let checked = |a: Result<String, Error>,
                   b: Result<(), Error>,
                   c: Result<String, Error>,
                   d: Result<(), Error>| {
        assert_eq!(a.unwrap(), "{\"from\":\"taplo\"}");
        b.unwrap();
        assert!(matches!(c, Err(Error::Denied(_))), "{c:?}");
        assert!(matches!(d, Err(Error::Denied(_))), "{d:?}");
    };
    let (a, b, c, d, owner, other) = round(&mut s, owner, other);
    checked(a, b, c, d);

    // Its arguments change: the user is asked again, and the granted server keeps running.
    let mut changed = granted.clone();
    changed.spec.args.push("--verbose".into());
    s.cx.update(|_, cx| crate::set_extension_servers(cx, vec![changed]));
    s.draw();
    assert_eq!(
        s.cx.update(|_, cx| crate::pending_extension_consents(cx))
            .len(),
        1
    );
    let (a, b, c, d, _, _) = round(&mut s, owner, other);
    checked(a, b, c, d);
    let notified = s
        .fake
        .records()
        .iter()
        .filter(|record| matches!(record, Record::Notification { method, .. } if method == NOTICE))
        .count();
    assert_eq!(notified, 2, "the owner's two notifications reached taplo");
}

/// A custom notification takes no request id: after one, cancelling a waiting request names the
/// id that request went out with on the wire.
#[gpui::test]
fn a_cancelled_request_after_a_notification_is_cancelled_by_its_wire_id(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: Reply::Result(json!(null)),
            delay: Duration::from_secs(600),
        },
    );
    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    let requests = LspRequests::new();
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(60),
        PathBuf::new(),
        Arc::default(),
    );
    let mut plugin = HostState {
        lsp: State::new("ids", &grants(), Vec::new(), &requests, sessions),
        ..HostState::default()
    };
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let thread = thread::spawn(move || {
        let target = || Target::Root(root.clone());
        plugin
            .notify(
                "rust-analyzer".into(),
                target(),
                NOTICE.into(),
                "null".into(),
            )
            .unwrap();
        plugin.request(
            "rust-analyzer".into(),
            target(),
            METHOD.into(),
            "null".into(),
        )
    });
    let until = Instant::now() + Duration::from_secs(60);
    let request_id = loop {
        s.draw();
        let found = s
            .fake
            .records()
            .into_iter()
            .find_map(|record| match record {
                Record::Request { id, method, .. } if method == METHOD => Some(id),
                _ => None,
            });
        if let Some(id) = found {
            break id;
        }
        assert!(Instant::now() < until, "the server never got the request");
        thread::sleep(Duration::from_millis(2));
    };
    // The notification went out too, before the request.
    let timeline = s.fake.records();
    let notice = timeline.iter().position(
        |record| matches!(record, Record::Notification { method, .. } if method == NOTICE),
    );
    let request = timeline
        .iter()
        .position(|record| matches!(record, Record::Request { method, .. } if method == METHOD));
    assert!(notice < request, "the notification went out first");

    // Unloading drops the waiting request, which cancels it.
    requests.unload("ids");
    let cancelled = loop {
        s.draw();
        let found = s
            .fake
            .records()
            .into_iter()
            .find_map(|record| match record {
                Record::Notification { method, params } if method == "$/cancelRequest" => {
                    Some(params["id"].clone())
                }
                _ => None,
            });
        if let Some(id) = found {
            break id;
        }
        assert!(Instant::now() < until, "no $/cancelRequest was sent");
        thread::sleep(Duration::from_millis(2));
    };
    let wire = serde_json::to_value(&request_id).unwrap();
    assert_eq!(cancelled, wire, "the cancel names the request's wire id");
    assert!(matches!(thread.join().unwrap(), Err(Error::Denied(_))));
}

/// A request of a plugin whose load is cancelled while the request waits on a slow server (as an
/// `init` that asks the server is, when the plugin is unloaded) stops at once, and the server's
/// request is cancelled: the load's flag reaches the call, though the plugin isn't loaded yet.
#[gpui::test]
fn cancelling_a_load_drops_a_request_waiting_on_a_slow_server(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: Reply::Result(json!(null)),
            delay: Duration::from_secs(600),
        },
    );
    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    let load = Arc::new(AtomicBool::new(false));
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(60),
        PathBuf::new(),
        load.clone(),
    );
    let mut plugin = HostState {
        lsp: State::new("init", &grants(), Vec::new(), &LspRequests::new(), sessions),
        ..HostState::default()
    };
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let call = thread::spawn(move || {
        plugin.request(
            "rust-analyzer".into(),
            Target::Root(root),
            METHOD.into(),
            "null".into(),
        )
    });
    let until = Instant::now() + Duration::from_secs(60);
    while !s
        .fake
        .records()
        .iter()
        .any(|record| matches!(record, Record::Request { method, .. } if method == METHOD))
    {
        assert!(
            Instant::now() < until,
            "the request never reached the server"
        );
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    // What unloading a plugin that is still loading does.
    let started = Instant::now();
    load.store(true, Ordering::Release);
    while !call.is_finished() {
        assert!(
            Instant::now() < until,
            "the request of a cancelled load kept waiting"
        );
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    assert!(started.elapsed() < Duration::from_secs(10));
    let result = call.join().unwrap();
    assert!(matches!(result, Err(Error::Denied(_))), "{result:?}");
    let cancelled = s.fake.records().iter().any(
        |record| matches!(record, Record::Notification { method, .. } if method == "$/cancelRequest"),
    );
    assert!(cancelled, "the server's request was cancelled");
}

/// A notification of a plugin whose load was cancelled is never sent, though the main thread
/// takes its job only after.
#[gpui::test]
fn a_notification_of_a_cancelled_load_is_not_sent(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    let load = Arc::new(AtomicBool::new(false));
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(30),
        PathBuf::new(),
        load.clone(),
    );
    let mut plugin = HostState {
        lsp: State::new(
            "loading",
            &grants(),
            Vec::new(),
            &LspRequests::new(),
            sessions,
        ),
        ..HostState::default()
    };
    // Cancelled before it calls: the call stops before anything is queued or sent.
    load.store(true, Ordering::Release);
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");
    let result = on_plugin_thread(&mut s, move || {
        plugin.notify(
            "rust-analyzer".into(),
            Target::Root(root),
            NOTICE.into(),
            "null".into(),
        )
    });
    assert!(matches!(result, Err(Error::Denied(_))), "{result:?}");
    for _ in 0..30 {
        s.draw();
        thread::sleep(Duration::from_millis(2));
    }
    let sent =
        s.fake.records().iter().any(
            |record| matches!(record, Record::Notification { method, .. } if method == NOTICE),
        );
    assert!(!sent, "the cancelled load's notification was sent");
}

/// A notification to a server that has stopped for good, whose handle the manager still holds,
/// is `not-found`, not a success that goes nowhere.
#[gpui::test]
fn a_notification_to_a_stopped_session_is_not_found(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, Script::default(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let (bridge, jobs) = Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    s.cx.update(|_, cx| serve(jobs, &commands, cx));
    let sessions = bridge.lsp(
        CallDeadline::default(),
        Duration::from_secs(30),
        PathBuf::new(),
        Arc::default(),
    );
    let mut plugin = HostState {
        lsp: State::new(
            "stopped",
            &grants(),
            Vec::new(),
            &LspRequests::new(),
            sessions,
        ),
        ..HostState::default()
    };
    let root = paths::root(&s.root, paths::Rules::HOST).expect("a guest root");

    // It crashes more often than it is restarted (5 times in 3 minutes): it stops for good.
    s.fake.wait_initialized();
    for crash in 1..=6usize {
        s.fake.crash_at(crash);
        if crash < 6 {
            let backoff = Duration::from_secs(1 << (crash - 1));
            s.clock.wait_for_sleep(backoff);
            s.clock.advance(backoff);
            s.fake.wait_initialized_at(crash + 1);
        }
    }
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        s.draw();
        let (result, back) = {
            let root = root.clone();
            on_plugin_thread(&mut s, move || {
                let result = plugin.notify(
                    "rust-analyzer".into(),
                    Target::Root(root),
                    NOTICE.into(),
                    "null".into(),
                );
                (result, plugin)
            })
        };
        plugin = back;
        match result {
            Err(Error::NotFound(_)) => break,
            // The server's thread is still ending: ask again.
            Ok(()) => {}
            other => panic!("{other:?}"),
        }
        assert!(
            Instant::now() < until,
            "a notification to a stopped server still succeeds"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
