// SPDX-License-Identifier: LicenseRef-Proprietary

//! A workspace folder's own extensions, `<root>/.notes/extensions`, load
//! beside the user's only while the root is trusted, headless: untrusted,
//! they're skipped; trusting the root loads them, an edit reloads them,
//! and withdrawing trust or changing the root drops them. The user's copy
//! of an ID wins. The batches are sent as the tree watcher would send them.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::TestAppContext;
use kiln::{
    Config,
    core::{
        ApiVersion,
        extension::{ExtensionSource, MANIFEST_FILE},
        trust::canonical,
    },
    platform::config::{self, BatchSender, batch_channel},
    shell::trust,
};
use serde_json::json;

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

fn install(extensions: &Path, dir: &str, id: &str, command: &str) {
    let dir = extensions.join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\nnotes = \"1.0\"\n\n[contributes.lsp]\ncommand = \"{command}\"\n"
        ),
    )
    .unwrap();
}

/// Each call's extensions: source, ID and `lsp` command.
type Seen = Rc<RefCell<Vec<Vec<(ExtensionSource, String, String)>>>>;

struct Host {
    /// The user's extensions directory.
    user: PathBuf,
    /// The workspace folder, canonical.
    root: PathBuf,
    /// Its extensions directory.
    extensions: PathBuf,
    seen: Seen,
    sender: BatchSender,
    _state: tempfile::TempDir,
}

/// kiln over a user's extensions directory, empty, taking a workspace's
/// `lsp` contributions, and a workspace folder with `acme.zig`, its root
/// not set yet.
fn host(cx: &mut TestAppContext) -> Host {
    let state = tempfile::tempdir().unwrap();
    let user = state.path().join("extensions");
    let root = canonical(&state.path().join("project"));
    fs::create_dir_all(&root).unwrap();
    let root = canonical(&root);
    let extensions = root.join(".notes").join("extensions");
    install(&extensions, "zig", "acme.zig", "zls");
    let seen = Seen::default();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .extensions(&user, "notes", ApiVersion { major: 1, minor: 0 })
        .extension_kind(
            "lsp",
            json!({ "type": "object", "properties": { "command": { "type": "string" } } }),
        )
        .workspace_extensions(["lsp"])
        .on_extensions({
            let seen = seen.clone();
            move |set, _| {
                let loaded = set.sourced().map(|(source, manifest)| {
                    let command = manifest.app["lsp"]["command"].as_str().unwrap_or_default();
                    (source, manifest.id.clone(), command.to_owned())
                });
                seen.borrow_mut().push(loaded.collect());
            }
        })
        .watch_with(batches);
    cx.update(|cx| {
        trust::init(cx);
        assert_eq!(kiln::init(cx, config), []);
    });
    cx.run_until_parked();
    Host {
        user,
        root,
        extensions,
        seen,
        sender,
        _state: state,
    }
}

impl Host {
    fn set_root(&self, root: Option<&Path>, cx: &mut TestAppContext) {
        cx.update(|cx| kiln::extensions::set_workspace_root(cx, root.map(Path::to_owned)));
        cx.run_until_parked();
    }

    fn decide(&self, trusted: bool, cx: &mut TestAppContext) {
        cx.update(|cx| trust::decide(&self.root, trusted, cx));
        cx.run_until_parked();
    }

    /// What `on_extensions` was last called with.
    fn last_seen(&self) -> Vec<(ExtensionSource, String, String)> {
        self.seen.borrow().last().cloned().unwrap_or_default()
    }

    /// Whether `on_extensions` was ever called with `id`.
    fn ever_seen(&self, id: &str) -> bool {
        (self.seen.borrow().iter().flatten()).any(|(_, seen, _)| seen == id)
    }
}

fn loaded(cx: &mut TestAppContext) -> Vec<(ExtensionSource, String)> {
    cx.update(|cx| kiln::extensions::loaded_from(cx))
        .into_iter()
        .map(|(source, manifest)| (source, manifest.id))
        .collect()
}

fn skipped(cx: &mut TestAppContext) -> Vec<(String, String)> {
    cx.update(|cx| kiln::extensions::skipped(cx))
        .into_iter()
        .map(|skipped| (skipped.manifest.id, skipped.reason))
        .collect()
}

#[gpui::test]
fn an_untrusted_root_is_skipped_and_contributes_nothing(cx: &mut TestAppContext) {
    let host = host(cx);
    let reason = format!("{} isn't trusted", host.root.display());

    // Undecided is untrusted.
    host.set_root(Some(&host.root), cx);
    assert_eq!(skipped(cx), [("acme.zig".to_owned(), reason.clone())]);
    assert_eq!(loaded(cx), []);

    host.decide(false, cx);
    assert_eq!(skipped(cx), [("acme.zig".to_owned(), reason)]);
    assert_eq!(loaded(cx), []);
    // A change under its directory isn't read either.
    install(&host.extensions, "odin", "acme.odin", "ols");
    host.sender.send(vec![host.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(loaded(cx), []);
    assert!(!host.seen.borrow().is_empty(), "on_extensions was called");
    assert!(!host.ever_seen("acme.zig") && !host.ever_seen("acme.odin"));
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert!(
        !(diagnostics.iter()).any(|d| d.file.starts_with(&host.extensions)),
        "{diagnostics:#?}"
    );
}

#[gpui::test]
fn trusting_loads_and_withdrawing_trust_drops_it(cx: &mut TestAppContext) {
    let host = host(cx);
    host.set_root(Some(&host.root), cx);
    assert_eq!(loaded(cx), []);

    host.decide(true, cx);
    let zig = |command: &str| {
        (
            ExtensionSource::Workspace,
            "acme.zig".into(),
            command.into(),
        )
    };
    assert_eq!(host.last_seen(), [zig("zls")]);
    assert_eq!(
        loaded(cx),
        [(ExtensionSource::Workspace, "acme.zig".into())]
    );
    assert_eq!(skipped(cx), []);

    // An edit to its manifest while trusted reloads it.
    install(&host.extensions, "zig", "acme.zig", "zls-nightly");
    host.sender.send(vec![host.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(host.last_seen(), [zig("zls-nightly")]);

    // Revoking trust withdraws it.
    host.decide(false, cx);
    assert_eq!(host.last_seen(), []);
    assert_eq!(loaded(cx), []);
    assert_eq!(skipped(cx).len(), 1);
    // And an edit then isn't read.
    install(&host.extensions, "zig", "acme.zig", "zls-old");
    host.sender.send(vec![host.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(loaded(cx), []);

    // Trusted again, it's read as it is now.
    host.decide(true, cx);
    assert_eq!(host.last_seen(), [zig("zls-old")]);

    // Changing the root withdraws it.
    let other = host.root.parent().unwrap().join("other");
    fs::create_dir_all(&other).unwrap();
    host.set_root(Some(&other), cx);
    assert_eq!(host.last_seen(), []);
    assert_eq!(loaded(cx), []);
    host.set_root(Some(&host.root), cx);
    assert_eq!(host.last_seen(), [zig("zls-old")]);
    host.set_root(None, cx);
    assert_eq!(host.last_seen(), []);
    assert_eq!((loaded(cx), skipped(cx)), (vec![], vec![]));
}

#[gpui::test]
fn the_users_copy_of_an_id_wins(cx: &mut TestAppContext) {
    let host = host(cx);
    install(&host.user, "zig", "acme.zig", "user-zls");
    host.sender.send(vec![host.user.clone()]);
    cx.run_until_parked();
    install(&host.extensions, "odin", "acme.odin", "ols");
    host.decide(true, cx);
    host.set_root(Some(&host.root), cx);

    assert_eq!(
        host.last_seen(),
        [
            (ExtensionSource::User, "acme.zig".into(), "user-zls".into()),
            (ExtensionSource::Workspace, "acme.odin".into(), "ols".into()),
        ]
    );
    let loaded = cx.update(|cx| kiln::extensions::loaded_from(cx));
    assert_eq!(loaded[0].1.dir, host.user.join("zig"));
    let workspace_zig = host.extensions.join("zig");
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    let warnings: Vec<_> = (diagnostics.iter())
        .filter(|d| d.file == workspace_zig.join(MANIFEST_FILE))
        .collect();
    let [warning] = warnings[..] else {
        panic!("one warning for the workspace's acme.zig: {diagnostics:#?}");
    };
    assert!(!warning.is_error());
    assert_eq!(
        warning.message,
        format!(
            "the workspace's acme.zig in {} isn't loaded: the user's copy in {} loads instead",
            workspace_zig.display(),
            host.user.join("zig").display()
        )
    );
}
