//! The package an extension's language server consent binds is watched by
//! entry: a retargeted directory link, an added or removed directory or an
//! oversized file asks again, through the real watcher.

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

fn pending(cx: &mut TestAppContext) -> Vec<fe_workbench::PendingConsent> {
    cx.update(|cx| fe_workbench::pending_extension_consents(cx))
}

/// Grants what's pending, settles, and returns the servers granted.
fn grant_all(cx: &mut TestAppContext) {
    for pending in pending(cx) {
        cx.update(|cx| fe_workbench::grant_extension_server(cx, &pending))
            .unwrap();
    }
    cx.run_until_parked();
}

/// A loaded, granted package with `server.js`, whose arguments and manifest
/// never change below.
fn granted_package(cx: &mut TestAppContext, state: &Path) -> (BatchSender, std::path::PathBuf) {
    let extensions = state.join("extensions");
    install(&extensions, "[\"server.js\"]");
    let zig = extensions.join("zig");
    fs::write(zig.join("server.js"), "require('./a/m')\n").unwrap();
    for name in ["a", "b"] {
        fs::create_dir_all(zig.join("mods").join(name)).unwrap();
        fs::write(zig.join("mods").join(name).join("m.js"), name).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink("mods/a", zig.join("a")).unwrap();
    let sender = load(cx, state);
    grant_all(cx);
    assert_eq!(pending(cx), []);
    (sender, extensions)
}

/// Sends a batch and returns the one pending consent it asks.
fn asks_once(
    cx: &mut TestAppContext,
    sender: &BatchSender,
    extensions: &Path,
    what: &str,
) -> fe_workbench::PendingConsent {
    sender.send(vec![extensions.to_owned()]);
    cx.run_until_parked();
    let mut asked = pending(cx);
    assert_eq!(asked.len(), 1, "{what}: {asked:#?}");
    asked.remove(0)
}

#[cfg(unix)]
#[gpui::test]
fn a_retargeted_directory_symlink_asks_consent_again(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (sender, extensions) = granted_package(cx, state.path());
    let link = extensions.join("zig/a");
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("mods/b", &link).unwrap();
    let asked = asks_once(cx, &sender, &extensions, "a retargeted link");
    let previous = asked.previous.as_ref().unwrap();
    assert_eq!(asked.invocation.args, previous.args);
    assert_eq!(asked.invocation.script_hashes, previous.script_hashes);
    assert_ne!(asked.invocation.package_hash, previous.package_hash);
}

#[gpui::test]
fn an_added_or_removed_empty_directory_asks_consent_again(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (sender, extensions) = granted_package(cx, state.path());
    // Nothing changed: a batch asks nothing.
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(pending(cx), []);

    let empty = extensions.join("zig/mods/empty");
    fs::create_dir(&empty).unwrap();
    let added = asks_once(cx, &sender, &extensions, "an added directory");
    assert_ne!(
        added.invocation.package_hash,
        added.previous.as_ref().unwrap().package_hash
    );
    cx.update(|cx| fe_workbench::grant_extension_server(cx, &added))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(pending(cx), []);

    fs::remove_dir(&empty).unwrap();
    asks_once(cx, &sender, &extensions, "a removed directory");
}

/// Sparse: over the size consent allows, without the disk.
fn oversized(path: &Path) {
    fs::File::create(path)
        .unwrap()
        .set_len(fe_workbench::PACKAGE_MAX_FILE_BYTES + 1)
        .unwrap();
}

#[gpui::test]
fn an_added_oversized_file_reevaluates_consent(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (sender, extensions) = granted_package(cx, state.path());
    oversized(&extensions.join("zig/big.bin"));
    let asked = asks_once(cx, &sender, &extensions, "an added oversized file");
    assert_eq!(asked.invocation.package_hash, "incomplete");
    // Never granted: it stays asked, with no server started for it.
    assert!(
        cx.update(|cx| fe_workbench::grant_extension_server(cx, &asked))
            .is_err()
    );
    cx.run_until_parked();
    assert_eq!(pending(cx).len(), 1);
}

#[gpui::test]
fn a_removed_oversized_file_clears_the_incomplete_package(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    install(&extensions, "[\"server.js\"]");
    let zig = extensions.join("zig");
    fs::write(zig.join("server.js"), "1\n").unwrap();
    oversized(&zig.join("big.bin"));
    let sender = load(cx, state.path());
    let first = pending(cx);
    assert_eq!(first.len(), 1, "{first:#?}");
    assert_eq!(first[0].invocation.package_hash, "incomplete");

    fs::remove_file(zig.join("big.bin")).unwrap();
    let asked = asks_once(cx, &sender, &extensions, "a removed oversized file");
    assert_ne!(asked.invocation.package_hash, "incomplete");
    cx.update(|cx| fe_workbench::grant_extension_server(cx, &asked))
        .unwrap();
}
