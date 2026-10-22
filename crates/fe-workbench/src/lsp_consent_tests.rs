//! An extension's language server is withheld until the user consents to
//! its invocation, and asked about again when that changes (extensions
//! record §1, Q5).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};

use fe_lsp::{ReadinessMode, ServerSpec};
use gpui::TestAppContext;

use crate::lsp_consent::{ConsentGate, ConsentStore, Decision, Entry};
use crate::{
    CONSENT_FILE, ExtensionServer, Invocation, PendingConsent, decline_extension_server,
    extension_servers, grant_extension_server, pending_extension_consents,
    set_extension_consent_file, set_extension_servers,
};

/// An extension `acme.zig` in `dir`, with `server.js` beside its manifest.
fn extension(dir: &Path) -> std::path::PathBuf {
    let extension = dir.join("zig");
    fs::create_dir_all(&extension).unwrap();
    fs::write(extension.join("server.js"), "console.log(1)\n").unwrap();
    extension.canonicalize().unwrap()
}

/// `acme.zig`'s `zls`, run as `/usr/bin/node server.js --stdio`.
fn server(extension: &Path) -> ExtensionServer {
    ExtensionServer {
        extension_id: "acme.zig".into(),
        extension_name: "Zig".into(),
        extension_dir: extension.to_owned(),
        spec: ServerSpec {
            args: vec!["server.js".into(), "--stdio".into()],
            env: vec![("ZLS_LOG".into(), "info".into())],
            languages: vec!["zig".into()],
            ..ServerSpec::new("zls", "/usr/bin/node")
        },
    }
}

#[test]
fn invocation_covers_command_args_env_and_script_hash() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let spec = server(&extension).spec;
    let of = |spec: &ServerSpec| Invocation::of(spec, &extension, None);
    let base = of(&spec);
    let script = extension.join("server.js").to_string_lossy().into_owned();
    assert_eq!(base.command, Path::new("/usr/bin/node"));
    assert_eq!(base.args, ["server.js", "--stdio"]);
    assert_eq!(
        base.env,
        BTreeMap::from([("ZLS_LOG".into(), "info".into())])
    );
    assert_eq!(base.script_hashes.keys().collect::<Vec<_>>(), ["server.js"]);
    assert!(base.script_hashes["server.js"].ends_with(&format!(" {script}")));

    // The resolved executable, where the command is a bare name.
    let bare = ServerSpec {
        command: "node".into(),
        ..spec.clone()
    };
    assert_eq!(
        Invocation::of(&bare, &extension, None).command,
        Path::new("node")
    );
    assert_ne!(
        Invocation::of(&bare, &extension, Some(Path::new("/usr/bin/node"))),
        Invocation::of(&bare, &extension, Some(Path::new("/opt/node/bin/node")))
    );
    // An argument.
    let mut args = spec.clone();
    args.args.push("--verbose".into());
    assert_ne!(of(&args), base);
    // An environment variable, added or changed.
    let mut env = spec.clone();
    env.env
        .push(("NODE_OPTIONS".into(), "--require evil.js".into()));
    assert_ne!(of(&env), base);
    let mut env = spec.clone();
    env.env[0].1 = "debug".into();
    assert_ne!(of(&env), base);
    // The script's content, named alone or as a flag's value.
    fs::write(extension.join("server.js"), "console.log(2)\n").unwrap();
    assert_ne!(of(&spec), base);
    let flag = ServerSpec {
        args: vec!["--script=server.js".into()],
        ..spec.clone()
    };
    let before = of(&flag);
    assert_eq!(before.script_hashes.len(), 1);
    fs::write(extension.join("server.js"), "console.log(3)\n").unwrap();
    assert_ne!(of(&flag), before);
    // A file outside the extension's directory isn't hashed.
    fs::write(dir.path().join("outside.js"), "").unwrap();
    let outside = ServerSpec {
        args: vec!["../outside.js".into()],
        ..spec.clone()
    };
    assert!(of(&outside).script_hashes.is_empty());

    // What doesn't change what runs.
    let base = of(&spec);
    let other = ServerSpec {
        languages: vec!["zon".into()],
        settings: serde_json::json!({ "zls": { "enable_snippets": true } }),
        initialization_options: Some(serde_json::json!({ "a": 1 })),
        root_markers: vec!["build.zig".into()],
        readiness: ReadinessMode::ServerStatus,
        ..spec.clone()
    };
    assert_eq!(of(&other), base);
}

#[test]
fn interpreters_are_flagged() {
    let flagged = |command: &str| {
        Invocation::of(&ServerSpec::new("s", command), Path::new("/nowhere"), None)
            .runs_arbitrary_code()
    };
    for interpreter in [
        "node",
        "/usr/bin/python3",
        "sh",
        "/bin/bash",
        "deno",
        "python3.12",
    ] {
        assert!(flagged(interpreter), "{interpreter}");
    }
    for server in ["rust-analyzer", "/usr/local/bin/zls"] {
        assert!(!flagged(server), "{server}");
    }
}

/// A decision for server `name` of the extension in `dir`.
fn entry(dir: &str, name: &str, decision: Decision) -> Entry {
    Entry {
        extension_dir: dir.into(),
        server: name.into(),
        decision,
        invocation: Invocation {
            command: "/usr/bin/zls".into(),
            args: vec!["--stdio".into()],
            env: BTreeMap::from([("A".into(), "1".into())]),
            script_hashes: BTreeMap::from([("/ext/a.js".into(), "00ff".into())]),
        },
    }
}

#[test]
fn consent_store_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join(CONSENT_FILE);
    let granted = entry("/ext/zig", "zls", Decision::Granted);
    let declined = entry("/ext/odin", "ols", Decision::Declined);
    ConsentStore::new(&path).record(granted.clone()).unwrap();
    ConsentStore::new(&path).record(declined.clone()).unwrap();
    let read = ConsentStore::new(&path).load();
    assert_eq!(read.values().collect::<Vec<_>>(), [&declined, &granted]);
    // A later decision replaces the server's last.
    let regranted = entry("/ext/odin", "ols", Decision::Granted);
    ConsentStore::new(&path).record(regranted.clone()).unwrap();
    let read = ConsentStore::new(&path).load();
    assert_eq!(read.values().collect::<Vec<_>>(), [&regranted, &granted]);

    // A corrupt file reads as empty, with why.
    fs::write(&path, "[[server]]\nextension_dir = 1\n").unwrap();
    let error = ConsentStore::new(&path).read().unwrap_err();
    assert!(error.contains(CONSENT_FILE), "{error}");
    assert!(ConsentStore::new(&path).load().is_empty());

    // Two writers at once both land.
    fs::remove_file(&path).unwrap();
    let writers: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|writer| {
            let path = path.clone();
            thread::spawn(move || {
                for i in 0..20 {
                    let entry = entry(&format!("/ext/{writer}"), &i.to_string(), Decision::Granted);
                    ConsentStore::new(&path).record(entry).unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(ConsentStore::new(&path).load().len(), 40);
}

/// Gives `servers` and lets them be worked out.
fn set(cx: &mut TestAppContext, servers: Vec<ExtensionServer>) {
    cx.update(|cx| set_extension_servers(cx, servers));
    cx.run_until_parked();
}

/// The servers handed to the managers, by name, command and arguments.
fn handed(cx: &mut TestAppContext) -> Vec<ServerSpec> {
    cx.update(|cx| extension_servers(cx))
}

fn pending(cx: &mut TestAppContext) -> Vec<PendingConsent> {
    cx.update(|cx| pending_extension_consents(cx))
}

/// An app with its consent file in `dir`.
fn consenting(cx: &mut TestAppContext, dir: &Path) {
    let path = dir.join(CONSENT_FILE);
    cx.update(|cx| set_extension_consent_file(cx, path));
}

#[gpui::test]
fn an_unconsented_server_is_withheld_and_pending(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    consenting(cx, dir.path());
    set(cx, vec![server(&extension)]);

    assert_eq!(handed(cx), []);
    let pending = pending(cx);
    let expected = Invocation::of(&server(&extension).spec, &extension, None);
    assert_eq!(
        pending,
        [PendingConsent {
            extension_id: "acme.zig".into(),
            extension_name: "Zig".into(),
            extension_dir: extension.clone(),
            server: "zls".into(),
            invocation: expected,
            previous: None,
            declined: false,
        }]
    );
    assert!(pending[0].invocation.runs_arbitrary_code());
}

#[gpui::test]
fn granting_starts_it_and_persists(cx: &mut TestAppContext, later: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    consenting(cx, dir.path());
    set(cx, vec![server(&extension)]);
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    assert_eq!(handed(cx), [server(&extension).spec]);
    assert_eq!(pending(cx), []);

    // A later session, with the same file, starts it without asking.
    consenting(later, dir.path());
    set(later, vec![server(&extension)]);
    assert_eq!(handed(later), [server(&extension).spec]);
    assert_eq!(pending(later), []);

    // Declining keeps one out, and isn't asked about again.
    let odin = ExtensionServer {
        extension_id: "acme.odin".into(),
        spec: ServerSpec::new("ols", "/usr/bin/ols"),
        ..server(&extension)
    };
    set(cx, vec![server(&extension), odin.clone()]);
    let asked = pending(cx).remove(0);
    assert_eq!((asked.server.as_str(), asked.declined), ("ols", false));
    cx.update(|cx| decline_extension_server(cx, &asked))
        .unwrap();
    cx.run_until_parked();
    let declined = |cx: &mut TestAppContext| {
        assert_eq!(handed(cx), [server(&extension).spec]);
        let pending = pending(cx);
        assert_eq!(pending.len(), 1);
        assert_eq!(
            (pending[0].server.as_str(), pending[0].declined),
            ("ols", true)
        );
        assert_eq!(pending[0].invocation, asked.invocation);
    };
    declined(cx);
    set(cx, vec![server(&extension), odin.clone()]);
    declined(cx);
    set(later, vec![server(&extension), odin.clone()]);
    declined(later);
}

#[gpui::test]
fn a_changed_invocation_keeps_the_old_and_asks_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    consenting(cx, dir.path());
    let granted = server(&extension);
    set(cx, vec![granted.clone()]);
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    let invocation = asked.invocation;

    // Only its languages: no question, and the new spec.
    let mut languages = granted.clone();
    languages.spec.languages.push("zon".into());
    set(cx, vec![languages.clone()]);
    assert_eq!(handed(cx), [languages.spec.clone()]);
    assert_eq!(pending(cx), []);

    let keeps_the_granted = |cx: &mut TestAppContext, changed: &ExtensionServer| {
        let handed = handed(cx);
        assert_eq!(handed.len(), 1);
        assert_eq!(
            (&handed[0].command, &handed[0].args, &handed[0].env),
            (&granted.spec.command, &granted.spec.args, &granted.spec.env)
        );
        let pending = pending(cx);
        assert_eq!(pending.len(), 1, "{pending:#?}");
        assert_eq!(pending[0].previous.as_ref(), Some(&invocation));
        assert_eq!(
            pending[0].invocation,
            Invocation::of(&changed.spec, &extension, None)
        );
        assert!(!pending[0].declined);
    };
    // Its arguments.
    let mut args = languages.clone();
    args.spec.args.push("--verbose".into());
    set(cx, vec![args.clone()]);
    keeps_the_granted(cx, &args);
    // Its environment.
    let mut env = languages.clone();
    env.spec
        .env
        .push(("NODE_OPTIONS".into(), "--inspect".into()));
    set(cx, vec![env.clone()]);
    keeps_the_granted(cx, &env);
    // Its script's content.
    fs::write(extension.join("server.js"), "require('child_process')\n").unwrap();
    set(cx, vec![languages.clone()]);
    keeps_the_granted(cx, &languages);

    // Granting the change starts it as it now is.
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    assert_eq!(handed(cx), [languages.spec]);
    assert_eq!(pending(cx), []);
}

#[gpui::test]
fn a_later_session_runs_the_granted_only_while_its_script_is_as_granted(
    cx: &mut TestAppContext,
    later: &mut TestAppContext,
    last: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    consenting(cx, dir.path());
    let granted = server(&extension);
    set(cx, vec![granted.clone()]);
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();

    // Its arguments changed: the granted ones run, the absolute executable
    // as granted.
    let mut args = granted.clone();
    args.spec.args.push("--verbose".into());
    consenting(later, dir.path());
    set(later, vec![args.clone()]);
    let handed = handed(later);
    assert_eq!(handed.len(), 1);
    assert_eq!(
        (&handed[0].command, &handed[0].args, &handed[0].env),
        (&granted.spec.command, &granted.spec.args, &granted.spec.env)
    );
    assert_eq!(pending(later)[0].previous.as_ref(), Some(&asked.invocation));

    // Its script changed: what was granted can't run as it was, so nothing does.
    fs::write(extension.join("server.js"), "require('child_process')\n").unwrap();
    consenting(last, dir.path());
    set(last, vec![granted]);
    assert_eq!(self::handed(last), []);
    assert_eq!(pending(last)[0].previous.as_ref(), Some(&asked.invocation));
}

/// A launcher recording what it's asked to launch, and launching nothing.
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<ServerSpec>>);

impl fe_lsp::ServerLauncher for Recorder {
    fn launch(
        &self,
        spec: &ServerSpec,
        _root: &Path,
        _cancel: &fe_lsp::LaunchCancel,
    ) -> std::io::Result<fe_lsp::ServerIo> {
        self.0.lock().unwrap().push(spec.clone());
        Err(std::io::Error::other("recorded"))
    }
}

/// Launches `spec` through `gate`: whether it reached the host's
/// launcher, and with what command.
fn launch(gate: &ConsentGate, recorder: &Recorder, spec: &ServerSpec) -> Option<PathBuf> {
    let before = recorder.0.lock().unwrap().len();
    let result = fe_lsp::ServerLauncher::launch(
        gate,
        spec,
        Path::new("/project"),
        &fe_lsp::LaunchCancel::new(),
    );
    let launched = recorder
        .0
        .lock()
        .unwrap()
        .get(before)
        .map(|spec| spec.command.clone());
    match result {
        Err(error) if error.to_string() == "recorded" => assert!(launched.is_some()),
        Err(_) => assert!(launched.is_none()),
        Ok(_) => unreachable!(),
    }
    launched
}

/// Makes `path` an executable file.
fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Grants `server`'s pending invocation.
fn grant(cx: &mut TestAppContext, server: &ExtensionServer) {
    set(cx, vec![server.clone()]);
    let asked = pending(cx)
        .into_iter()
        .find(|pending| pending.server == server.spec.name)
        .expect("asked");
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn every_launch_is_checked_against_the_grant(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let (first, second) = (dir.path().join("a"), dir.path().join("b"));
    executable(&first.join("node"));
    executable(&second.join("node"));
    // `node` on the extension's PATH, found in `a` first.
    let path = format!("{}:{}", first.display(), second.display());
    let mut node = server(&extension);
    node.spec.command = "node".into();
    node.spec.env.push(("PATH".into(), path));
    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));

    // Granted: launched with the executable granted, not a name to look up.
    grant(cx, &node);
    assert_eq!(
        launch(&gate, &recorder, &node.spec),
        Some(first.join("node"))
    );
    // A spec of another command, a built-in's or the user's own, passes.
    let other = ServerSpec::new("zls", "/usr/bin/zls");
    assert_eq!(
        launch(&gate, &recorder, &other),
        Some(PathBuf::from("/usr/bin/zls"))
    );

    // Its script changes: a launch from now on, such as a crash's restart
    // or a new window's, is refused, though the spec is still handed.
    fs::write(extension.join("server.js"), "require('child_process')\n").unwrap();
    set(cx, vec![node.clone()]);
    assert_eq!(handed(cx).len(), 1);
    assert_eq!(launch(&gate, &recorder, &node.spec), None);
    fs::write(extension.join("server.js"), "console.log(1)\n").unwrap();
    assert_eq!(
        launch(&gate, &recorder, &node.spec),
        Some(first.join("node"))
    );

    // `node` on its PATH resolves elsewhere: refused, not run from `b`,
    // and asked about, with nothing given again.
    fs::remove_file(first.join("node")).unwrap();
    assert_eq!(launch(&gate, &recorder, &node.spec), None);
    cx.run_until_parked();
    let asked = pending(cx);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert_eq!(asked[0].invocation.command, second.join("node"));
    assert_eq!(
        asked[0].previous.as_ref().map(|previous| &previous.command),
        Some(&first.join("node"))
    );
    // Granted, it runs from `b`.
    cx.update(|cx| grant_extension_server(cx, &asked[0]))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        launch(&gate, &recorder, &node.spec),
        Some(second.join("node"))
    );
}

#[gpui::test]
fn a_retargeted_or_new_script_is_a_change(cx: &mut TestAppContext, later: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    fs::write(extension.join("a.js"), "a\n").unwrap();
    fs::write(extension.join("b.js"), "b\n").unwrap();
    std::os::unix::fs::symlink("a.js", extension.join("main.js")).unwrap();
    let mut node = server(&extension);
    // `lib.js` doesn't exist yet.
    node.spec.args = vec!["main.js".into(), "lib.js".into()];
    consenting(cx, dir.path());
    grant(cx, &node);
    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));
    assert!(launch(&gate, &recorder, &node.spec).is_some());

    let retarget = |extension: &Path| {
        fs::remove_file(extension.join("main.js")).unwrap();
        std::os::unix::fs::symlink("b.js", extension.join("main.js")).unwrap();
    };
    let create = |extension: &Path| fs::write(extension.join("lib.js"), "lib\n").unwrap();
    let changes: [&dyn Fn(&Path); 2] = [&retarget, &create];
    for change in changes {
        change(&extension);
        // Asked about, and refused at launch.
        set(cx, vec![node.clone()]);
        assert_eq!(pending(cx).len(), 1);
        assert_eq!(launch(&gate, &recorder, &node.spec), None);
        // A later session runs nothing of it.
        consenting(later, dir.path());
        set(later, vec![node.clone()]);
        assert_eq!(handed(later), []);
        assert_eq!(pending(later).len(), 1);
        // Put back, it runs again.
        fs::remove_file(extension.join("main.js")).unwrap();
        std::os::unix::fs::symlink("a.js", extension.join("main.js")).unwrap();
        fs::remove_file(extension.join("lib.js")).ok();
        set(cx, vec![node.clone()]);
        assert_eq!(pending(cx), []);
        assert!(launch(&gate, &recorder, &node.spec).is_some());
    }

    // A name pointed at another file that's named too, so the files hashed
    // are the same: still a change.
    fs::write(extension.join("b.js"), "b\n").unwrap();
    let both = ServerSpec {
        args: vec!["main.js".into(), "a.js".into(), "b.js".into()],
        ..node.spec.clone()
    };
    let before = Invocation::of(&both, &extension, None);
    fs::remove_file(extension.join("main.js")).unwrap();
    std::os::unix::fs::symlink("b.js", extension.join("main.js")).unwrap();
    assert_ne!(Invocation::of(&both, &extension, None), before);
}

/// Gives `s`'s app `servers`, and draws.
fn give(s: &mut crate::tests::lsp::Lsp, servers: Vec<ExtensionServer>) {
    s.cx.update(|_, cx| set_extension_servers(cx, servers));
    s.draw();
}

/// Grants what `s`'s app asks about `name`.
fn grant_in(s: &mut crate::tests::lsp::Lsp, name: &str) {
    let asked = s.cx.update(|_, cx| pending_extension_consents(cx));
    let asked = asked
        .iter()
        .find(|pending| pending.server == name)
        .expect("asked");
    s.cx.update(|_, cx| grant_extension_server(cx, asked))
        .unwrap();
    s.draw();
}

/// `taplo`, an extension's TOML server running its `server.js`.
fn taplo(extension: &Path) -> ExtensionServer {
    ExtensionServer {
        spec: ServerSpec {
            args: vec!["server.js".into()],
            languages: vec!["toml".into()],
            ..ServerSpec::new("taplo", "/opt/taplo/bin/taplo")
        },
        ..server(extension)
    }
}

#[gpui::test]
fn a_running_server_restarts_only_once_its_change_is_granted(cx: &mut TestAppContext) {
    let mut s = crate::tests::lsp::Lsp::new(cx);
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    fs::write(s.path("a.toml"), "a = 1\n").unwrap();
    let granted = taplo(&extension);
    give(&mut s, vec![granted.clone()]);
    grant_in(&mut s, "taplo");
    s.open("a.toml");
    s.until("taplo to launch", |s| s.fake.launches() == 1);
    assert_eq!(s.fake.launched()[0].0.args, ["server.js"]);

    // Its arguments, languages and root markers change together: it's
    // asked about, and keeps running as it was, not restarted.
    let mut changed = granted.clone();
    changed.spec.args.push("--verbose".into());
    changed.spec.languages.push("json".into());
    changed.spec.root_markers.push("taplo.toml".into());
    give(&mut s, vec![changed.clone()]);
    s.draw();
    assert_eq!(s.fake.launches(), 1);
    assert_eq!(s.fake.kills(), 0);
    let handed = s.cx.update(|_, cx| extension_servers(cx));
    assert_eq!(handed, std::slice::from_ref(&granted.spec));

    // Granted, it restarts as it now is.
    grant_in(&mut s, "taplo");
    s.until("taplo to restart", |s| s.fake.launches() == 2);
    assert_eq!(s.fake.launched()[1].0.args, ["server.js", "--verbose"]);

    // Its script changes, with nothing else: still running, asked about.
    fs::write(extension.join("server.js"), "changed\n").unwrap();
    give(&mut s, vec![changed.clone()]);
    assert_eq!(s.fake.launches(), 2);
    // Granted, the old script stops and the new one starts.
    grant_in(&mut s, "taplo");
    s.until("taplo to restart", |s| s.fake.launches() == 3);
}

#[gpui::test]
fn a_launch_refused_for_a_change_starts_once_it_is_granted(cx: &mut TestAppContext) {
    let mut s = crate::tests::lsp::Lsp::new(cx);
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    fs::write(s.path("a.toml"), "a = 1\n").unwrap();
    let granted = taplo(&extension);
    give(&mut s, vec![granted.clone()]);
    grant_in(&mut s, "taplo");

    // Its script changes before anything opened it: the spec is still
    // handed, but the server it would start is refused.
    fs::write(extension.join("server.js"), "changed\n").unwrap();
    give(&mut s, vec![granted.clone()]);
    s.open("a.toml");
    s.draw();
    assert_eq!(s.fake.launches(), 0);

    // Granted, it starts.
    grant_in(&mut s, "taplo");
    s.until("taplo to launch", |s| s.fake.launches() == 1);
}

#[gpui::test]
fn a_decision_is_on_disk_when_it_returns(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    consenting(cx, dir.path());
    let granted = server(&extension);
    grant(cx, &granted);
    let mut changed = granted.clone();
    changed.spec.args.push("--verbose".into());
    set(cx, vec![changed]);
    let asked = pending(cx).remove(0);

    // Declining the change replaces the grant in the file at once, so a
    // quit straight after can't bring the grant back.
    let store = ConsentStore::new(dir.path().join(CONSENT_FILE));
    cx.update(|cx| {
        decline_extension_server(cx, &asked).unwrap();
        let entries = store.read().unwrap();
        let entry = &entries[&(extension.clone(), "zls".to_owned())];
        assert_eq!(entry.decision, Decision::Declined);
        assert_eq!(entry.invocation, asked.invocation);
    });
    // And what was granted stops.
    cx.run_until_parked();
    assert_eq!(handed(cx), []);

    // A write that fails is returned, and the decision holds for the session.
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "").unwrap();
    cx.update(|cx| set_extension_consent_file(cx, blocked.join(CONSENT_FILE)));
    set(cx, vec![granted.clone()]);
    let asked = pending(cx).remove(0);
    assert!(cx.update(|cx| grant_extension_server(cx, &asked)).is_err());
    cx.run_until_parked();
    assert_eq!(handed(cx), [granted.spec]);
}

#[gpui::test]
fn only_the_first_server_of_a_name_is_handed_and_checked(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    let other = other.canonicalize().unwrap();
    // Two extensions' `zls`, both running the first's script, which is
    // outside the second's directory, so its grant doesn't hash it.
    let script = extension.join("server.js").to_string_lossy().into_owned();
    let mut first = server(&extension);
    first.spec.args = vec![script.clone()];
    let second = ExtensionServer {
        extension_id: "acme.other".into(),
        extension_dir: other.clone(),
        ..first.clone()
    };
    set(cx, vec![first.clone(), second.clone()]);
    for asked in pending(cx) {
        cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    }
    cx.run_until_parked();
    assert_eq!(handed(cx), [first.spec.clone()]);
    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));
    assert!(launch(&gate, &recorder, &first.spec).is_some());

    // The script changes: the first's grant refuses it, and the second's,
    // which would pass, isn't consulted.
    fs::write(extension.join("server.js"), "changed\n").unwrap();
    assert_eq!(launch(&gate, &recorder, &first.spec), None);
    cx.run_until_parked();
    let asked = pending(cx);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert_eq!(asked[0].extension_dir, extension);
    // Granted, it runs.
    cx.update(|cx| grant_extension_server(cx, &asked[0]))
        .unwrap();
    cx.run_until_parked();
    assert!(launch(&gate, &recorder, &first.spec).is_some());
}

/// A launcher that fails or launches `fake`'s server, as `fail` says.
struct Flaky {
    fake: Arc<fe_lsp::fake::FakeLauncher>,
    fail: std::sync::atomic::AtomicBool,
    /// Blocks a launch until a message, while set.
    hold: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl fe_lsp::ServerLauncher for Flaky {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &fe_lsp::LaunchCancel,
    ) -> std::io::Result<fe_lsp::ServerIo> {
        let hold = self.hold.lock().unwrap().take();
        if let Some(hold) = hold {
            hold.recv().unwrap();
        }
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::other("transient"));
        }
        self.fake.launch(spec, root, cancel)
    }
}

#[gpui::test]
fn a_launch_that_failed_ended_or_was_under_way_restarts_once_a_change_is_granted(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let fake = fe_lsp::fake::FakeServer::new(fe_lsp::fake::Script::default());
    let flaky = Arc::new(Flaky {
        fake: fake.launcher(),
        fail: true.into(),
        hold: Default::default(),
    });
    let gate = Arc::new(cx.update(|cx| ConsentGate::new(cx, flaky.clone())));
    let granted = server(&extension);
    grant(cx, &granted);
    let root = Path::new("/project");
    let launch = |gate: &ConsentGate| {
        fe_lsp::ServerLauncher::launch(gate, &granted.spec, root, &fe_lsp::LaunchCancel::new())
    };
    let at = vec![("zls".to_owned(), root.to_owned())];
    // Grants a change of the script, and gives what the gate would restart.
    let mut version = 0;
    let mut change = |cx: &mut TestAppContext, gate: &ConsentGate| {
        version += 1;
        fs::write(extension.join("server.js"), format!("v{version}\n")).unwrap();
        set(cx, vec![granted.clone()]);
        let asked = pending(cx).remove(0);
        cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
        cx.run_until_parked();
        gate.stale()
    };

    // A launch that failed: nothing to restart until a change is granted.
    assert_eq!(launch(&gate).err().unwrap().to_string(), "transient");
    assert_eq!(gate.stale(), []);
    assert_eq!(change(cx, &gate), at);
    assert_eq!(gate.stale(), []);

    // One that ran and ended, as after its crashes ran out.
    flaky.fail.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(launch(&gate).unwrap());
    assert_eq!(gate.stale(), []);
    assert_eq!(change(cx, &gate), at);

    // One running: restarted once a change is granted, not before.
    let running = launch(&gate).unwrap();
    assert_eq!(gate.stale(), []);
    assert_eq!(change(cx, &gate), at);
    drop(running);

    // One under way as a change is granted, checked against the grant
    // before: once it lands, the app is woken and it's given.
    let (release, hold) = std::sync::mpsc::channel();
    *flaky.hold.lock().unwrap() = Some(hold);
    let launching = {
        let gate = gate.clone();
        let spec = granted.spec.clone();
        thread::spawn(move || {
            fe_lsp::ServerLauncher::launch(&*gate, &spec, root, &fe_lsp::LaunchCancel::new())
                .map(drop)
        })
    };
    while flaky.hold.lock().unwrap().is_some() {
        thread::yield_now();
    }
    // The last one landed, which ended, is given now.
    assert_eq!(change(cx, &gate), at);
    release.send(()).unwrap();
    launching.join().unwrap().unwrap();
    assert_eq!(gate.stale(), at);
}
