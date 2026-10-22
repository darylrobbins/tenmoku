//! One consent grant relaunches the server it changes once, not twice.

use std::{fs, path::Path};

use fe_lsp::{
    ServerSpec,
    fake::{OnExit, Script},
};

use crate::{
    ExtensionServer, grant_extension_server, pending_extension_consents, set_extension_servers,
};

type Lsp = crate::tests::lsp::Lsp;

/// An extension in `dir`, with `server.js` beside its manifest.
fn extension(dir: &Path) -> std::path::PathBuf {
    let extension = dir.join("toml");
    fs::create_dir_all(&extension).unwrap();
    fs::write(extension.join("server.js"), "console.log(1)\n").unwrap();
    extension.canonicalize().unwrap()
}

/// `taplo`, the extension's TOML server running its `server.js`.
fn taplo(extension: &Path) -> ExtensionServer {
    ExtensionServer {
        extension_id: "acme.toml".into(),
        extension_name: "Toml".into(),
        extension_version: "1.0.0".into(),
        extension_license: "MIT".into(),
        extension_dir: extension.to_owned(),
        spec: ServerSpec {
            args: vec!["server.js".into()],
            languages: vec!["toml".into()],
            ..ServerSpec::new("taplo", "/opt/taplo/bin/taplo")
        },
    }
}

/// Gives `s`'s app `servers`, and draws.
fn give(s: &mut Lsp, servers: Vec<ExtensionServer>) {
    s.cx.update(|_, cx| set_extension_servers(cx, servers));
    s.draw();
}

/// Grants what `s`'s app asks about `name`.
fn grant_in(s: &mut Lsp, name: &str) {
    let asked = s.cx.update(|_, cx| pending_extension_consents(cx));
    let asked = asked
        .iter()
        .find(|pending| pending.server == name)
        .expect("asked");
    s.cx.update(|_, cx| grant_extension_server(cx, asked))
        .unwrap();
    s.draw();
}

#[gpui::test]
fn one_grant_relaunches_the_server_exactly_once(cx: &mut gpui::TestAppContext) {
    // The old server ignores shutdown and exit, so it ends only by being
    // killed, once the fake clock passes the shutdown and exit timeouts.
    let stubborn = Script {
        answer_shutdown: false,
        on_exit: OnExit::Ignore,
        ..Script::default()
    };
    let mut s = crate::tests::lsp::Lsp::with_fake_clock(cx, stubborn, Script::default());
    s.set_trust(true);
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    fs::write(s.path("a.toml"), "a = 1\n").unwrap();
    let granted = taplo(&extension);
    give(&mut s, vec![granted.clone()]);
    grant_in(&mut s, "taplo");
    s.open("a.toml");
    s.until("taplo to launch", |s| s.fake.launches() == 1);
    // The first launch has been recorded by the gate once the server has
    // the document: a launch count alone is seen before the gate records.
    s.fake.wait_for("the first launch's didOpen", |records| {
        records
            .iter()
            .any(|record| record.method() == "textDocument/didOpen")
    });
    let first = s.active_server().flatten().expect("on a server");

    let mut changed = granted.clone();
    changed.spec.args.push("--verbose".into());
    give(&mut s, vec![changed]);
    grant_in(&mut s, "taplo");
    s.until("the old taplo to be killed and restarted", |s| {
        s.clock.advance(std::time::Duration::from_secs(3));
        s.fake.kills() >= 1 && s.fake.launches() >= 2
    });
    s.cx.run_until_parked();

    // The grant starts one instance in place of the old one, however far
    // the first got before a second restart replaced it: instances are
    // numbered as they are made, so a second restart is a second number.
    let now = s.active_server().flatten().expect("on a server");
    assert_eq!(format!("{now:?}"), next(first));

    // And that one instance launched once, as granted, with nothing
    // launching or killed after the executor parks: given a moment to.
    let settled = std::time::Instant::now() + std::time::Duration::from_millis(300);
    while std::time::Instant::now() < settled && s.fake.launches() == 2 {
        s.draw();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(s.fake.launches(), 2);
    assert_eq!(s.fake.launched()[1].0.args, ["server.js", "--verbose"]);
    assert_eq!(s.fake.kills(), 1, "only the old server is killed");
}

/// The id after `id`, as `ServerId`'s debug form spells it.
fn next(id: fe_lsp::ServerId) -> String {
    let text = format!("{id:?}");
    let number: u64 = text
        .trim_start_matches("ServerId(")
        .trim_end_matches(')')
        .parse()
        .unwrap();
    format!("ServerId({})", number + 1)
}

#[gpui::test]
fn a_late_record_of_the_replaced_launch_does_not_relaunch_its_replacement(
    cx: &mut gpui::TestAppContext,
) {
    let mut s = Lsp::new(cx);
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    fs::write(s.path("a.toml"), "a = 1\n").unwrap();
    let granted = taplo(&extension);
    give(&mut s, vec![granted.clone()]);
    grant_in(&mut s, "taplo");

    // The first launch is inside its launcher, not yet recorded by the gate.
    s.hold_launches_late(true);
    s.open("a.toml");
    s.wait_launch_held();
    let first = s.active_server().flatten().expect("on a server");

    // The changed spec is granted meanwhile: the launch is replaced.
    let mut changed = granted.clone();
    changed.spec.args.push("--verbose".into());
    give(&mut s, vec![changed]);
    grant_in(&mut s, "taplo");

    // The old launch then returns and is recorded late, and the
    // replacement, once it starts, waits in its launcher unrecorded.
    s.release_one_launch();
    s.until("the replacement to launch", |s| s.fake.launches() >= 2);
    s.wait_launch_held();
    for _ in 0..60 {
        s.draw();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // Nothing restarted the replacement meanwhile.
    let now = s.active_server().flatten().expect("on a server");
    assert_eq!(format!("{now:?}"), next(first));
    s.hold_launches_late(false);
    s.until("the replacement to run", |s| s.fake.launches() >= 2);
    s.cx.run_until_parked();
    assert_eq!(s.fake.launches(), 2);
    assert_eq!(s.fake.launched()[1].0.args, ["server.js", "--verbose"]);
}
