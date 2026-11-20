//! An extension's language server, loaded through fastedit's `lsp`
//! contribution kind, doesn't reach the workbench until the user consents
//! to what it runs, and is asked about again when a hot reload changes that
//! (extensions record §1, Q5).

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{fs, path::Path};

use fe_config::SettingsStore;
use gpui::TestAppContext;
use kiln::{
    Config,
    platform::config::{BatchSender, batch_channel},
    ui::ThemeMode,
};

/// The zig extension under `extensions`, its `zls` reading `zls.json`.
fn install(extensions: &Path, args: &str) {
    let dir = extensions.join("zig");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("zls.json"), "{}\n").unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"acme.zig\"\nname = \"Zig\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.0\"\n\n\
             [contributes.lsp.zls]\ncommand = \"zls\"\nargs = {args}\nlanguages = [\"zig\"]\n"
        ),
    )
    .unwrap();
}

/// fastedit's config with the extensions in `state/extensions`, its
/// consent file in `state`, loaded.
fn load(cx: &mut TestAppContext, state: &Path) -> BatchSender {
    let user = state.join("settings.toml");
    fs::write(&user, "").unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, state.join("extensions"));
    let consent = state.join(fe_workbench::CONSENT_FILE);
    cx.update(|cx| {
        fe_workbench::set_extension_consent_file(cx, consent);
        kiln::init(cx, config);
    });
    cx.run_until_parked();
    sender
}

/// The servers the workbench has, by name and arguments.
fn servers(cx: &mut TestAppContext) -> Vec<(String, Vec<String>)> {
    cx.update(|cx| fe_workbench::extension_servers(cx))
        .into_iter()
        .map(|spec| (spec.name, spec.args))
        .collect()
}

fn pending(cx: &mut TestAppContext) -> Vec<fe_workbench::PendingConsent> {
    cx.update(|cx| fe_workbench::pending_extension_consents(cx))
}

#[gpui::test]
fn an_installed_lsp_extension_is_pending_until_granted(
    cx: &mut TestAppContext,
    later: &mut TestAppContext,
) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    install(&extensions, "[\"--config-path\", \"zls.json\"]");
    let sender = load(cx, state.path());

    // Withheld, and asked about.
    assert_eq!(servers(cx), []);
    let asked = pending(cx);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    let asked = &asked[0];
    assert_eq!(
        (asked.extension_id.as_str(), asked.extension_name.as_str()),
        ("acme.zig", "Zig")
    );
    assert_eq!(asked.server, "zls");
    assert_eq!(asked.invocation.args, ["--config-path", "zls.json"]);
    assert_eq!(asked.invocation.script_hashes.len(), 1);
    assert_eq!(asked.previous, None);
    assert!(!asked.invocation.runs_arbitrary_code());

    // Granted, it reaches the workbench.
    cx.update(|cx| fe_workbench::grant_extension_server(cx, asked))
        .unwrap();
    cx.run_until_parked();
    let granted = vec![(
        "zls".to_owned(),
        vec!["--config-path".to_owned(), "zls.json".to_owned()],
    )];
    assert_eq!(servers(cx), granted);
    assert_eq!(pending(cx), []);

    // A file its arguments name changes on disk: it's asked about again,
    // the granted one kept.
    fs::write(
        extensions.join("zig/zls.json"),
        "{\"enable_build_on_save\": true}\n",
    )
    .unwrap();
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(servers(cx), granted);
    let again = pending(cx);
    assert_eq!(again.len(), 1, "{again:#?}");
    assert_eq!(again[0].previous.as_ref(), Some(&asked.invocation));

    // So do its arguments, in its manifest.
    install(
        &extensions,
        "[\"--config-path\", \"zls.json\", \"--enable-debug-log\"]",
    );
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(servers(cx), granted);
    let again = pending(cx);
    assert_eq!(again.len(), 1, "{again:#?}");
    assert_eq!(again[0].invocation.args.len(), 3);
    assert_eq!(again[0].previous.as_ref(), Some(&asked.invocation));

    // Put back as granted, a later launch starts it without asking.
    install(&extensions, "[\"--config-path\", \"zls.json\"]");
    load(later, state.path());
    assert_eq!(servers(later), granted);
    assert_eq!(pending(later), []);
}

#[gpui::test]
fn a_malformed_edit_keeps_the_last_good_server(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    install(&extensions, "[\"--config-path\", \"zls.json\"]");
    let sender = load(cx, state.path());
    for pending in pending(cx) {
        cx.update(|cx| fe_workbench::grant_extension_server(cx, &pending))
            .unwrap();
    }
    cx.run_until_parked();
    let granted = servers(cx);
    assert_eq!(granted.len(), 1);

    // An argument that isn't a string passes kiln's schema check, but
    // doesn't read as settings: the server keeps its last good ones and
    // doesn't stop. The manifest file itself changed, which is a change to
    // the installed package, so it asks again about the same invocation.
    install(&extensions, "[1]");
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(servers(cx), granted);
    let again = pending(cx);
    assert_eq!(again.len(), 1, "{again:#?}");
    let previous = again[0].previous.as_ref().unwrap();
    assert_eq!(again[0].invocation.args, previous.args);
    assert_ne!(again[0].invocation.package_hash, previous.package_hash);
}

#[gpui::test]
fn a_module_only_change_asks_again_through_the_watcher(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    // `server.js` imports `lib/util.js`; the manifest, script and arguments
    // never change below.
    install(&extensions, "[\"server.js\"]");
    let zig = extensions.join("zig");
    fs::create_dir_all(zig.join("lib")).unwrap();
    fs::write(zig.join("server.js"), "require('./lib/util')\n").unwrap();
    fs::write(zig.join("lib/util.js"), "exports.a = 1\n").unwrap();
    let sender = load(cx, state.path());
    for pending in pending(cx) {
        cx.update(|cx| fe_workbench::grant_extension_server(cx, &pending))
            .unwrap();
    }
    cx.run_until_parked();
    let granted = servers(cx);
    assert_eq!(granted.len(), 1);
    assert_eq!(pending(cx), []);

    // Nothing changed: a batch asks nothing.
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(pending(cx), []);

    let reasks = |cx: &mut TestAppContext, what: &str| {
        sender.send(vec![extensions.clone()]);
        cx.run_until_parked();
        assert_eq!(servers(cx), granted, "{what}");
        let again = pending(cx);
        assert_eq!(again.len(), 1, "{what}: {again:#?}");
        assert_eq!(
            again[0].invocation.args,
            again[0].previous.as_ref().unwrap().args
        );
        // Accepted, it's quiet again.
        cx.update(|cx| fe_workbench::grant_extension_server(cx, &again[0]))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(pending(cx), [], "{what}");
    };
    // A changed module.
    fs::write(zig.join("lib/util.js"), "exports.a = 2\n").unwrap();
    reasks(cx, "a changed module");
    // An added file.
    fs::write(zig.join("lib/extra.js"), "").unwrap();
    reasks(cx, "an added file");
    // A removed one.
    fs::remove_file(zig.join("lib/extra.js")).unwrap();
    reasks(cx, "a removed file");
}

#[test]
fn listing_a_package_stops_at_the_entry_limit() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        fs::create_dir_all(dir.path().join(name)).unwrap();
        fs::write(dir.path().join(name).join("m.js"), "").unwrap();
    }
    // Three directories and three files.
    assert_eq!(config::package_files_limited(dir.path(), 6).len(), 3);
    let capped = config::package_files_limited(dir.path(), 4);
    assert!(capped.len() < 3, "{capped:?}");
}

#[test]
fn listing_a_package_includes_a_big_file_so_its_addition_and_removal_are_seen() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("small.js"), "12345").unwrap();
    let before = config::package_files_limited(dir.path(), 100);
    assert_eq!(before, [dir.path().join("small.js")]);
    // Sparse: a file over the consent limit without the disk, only listed.
    let big = dir.path().join("big.bin");
    fs::File::create(&big)
        .unwrap()
        .set_len(fe_workbench::PACKAGE_MAX_FILE_BYTES + 1)
        .unwrap();
    let mut added = config::package_files_limited(dir.path(), 100);
    added.sort();
    assert_eq!(added, [big.clone(), dir.path().join("small.js")]);
    fs::remove_file(&big).unwrap();
    assert_eq!(config::package_files_limited(dir.path(), 100), before);
}
