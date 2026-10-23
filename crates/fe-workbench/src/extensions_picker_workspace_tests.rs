//! Extensions: Show Installed with a workspace folder's own extensions
//! (extensions record §3): a trusted folder's are listed with source
//! Workspace beside the user's; an untrusted folder's are listed skipped.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_lsp::ServerSpec;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{ApiVersion, extension::MANIFEST_FILE, trust::canonical},
    platform::config::batch_channel,
    shell::trust,
};
use serde_json::json;

use super::{ExtensionSource, ExtensionState, ExtensionsPicker, Row};
use crate::{
    ExtensionServer, Workbench,
    picker::{Picker, PickerDelegate as _},
    set_extension_servers,
    tests::open_root,
};

fn install(extensions: &Path, dir: &str, id: &str, name: &str) {
    let dir = extensions.join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "id = \"{id}\"\nname = \"{name}\"\nversion = \"1.2.3\"\nlicense = \"MIT\"\n\n\
             [engine]\nfastedit = \"1.0\"\n\n[contributes.lsp.zls]\ncommand = \"zls\"\n"
        ),
    )
    .unwrap();
}

/// kiln over a user's extensions directory with Zig, taking a workspace's
/// `lsp`, and a folder, `project`, with Odin in its own extensions
/// directory, set as the workspace root, its trust undecided. Returns the
/// folder, canonical.
fn host(dir: &Path, cx: &mut TestAppContext) -> std::path::PathBuf {
    let user = dir.join("extensions");
    install(&user, "zig", "acme.zig", "Zig");
    let root = dir.join("project");
    install(
        &root.join(".fastedit/extensions"),
        "odin",
        "team.odin",
        "Odin",
    );
    let root = canonical(&root);
    let (_sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .extensions(user, "fastedit", ApiVersion { major: 1, minor: 0 })
        .extension_kind("lsp", json!({ "type": "object" }))
        .workspace_extensions(["lsp"])
        // Not the real watcher, whose thread the test scheduler can't follow.
        .watch_with(batches);
    cx.update(|cx| {
        trust::init(cx);
        assert_eq!(kiln::init(cx, config), []);
        kiln::extensions::set_workspace_root(cx, Some(root.clone()));
    });
    cx.run_until_parked();
    root
}

fn workbench(root: &Path, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx.clone())
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Opens the picker afresh and returns its rows.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Row> {
    let run = RunCommand(
        Invocation::of("workbench.show_extensions")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let picker: Entity<Picker<ExtensionsPicker>> =
        workbench.read_with(cx, |workbench, _| workbench.extensions_picker().clone());
    picker.read_with(cx, |picker, _| picker.delegate.rows())
}

fn summary(rows: &[Row]) -> Vec<(String, ExtensionSource, ExtensionState)> {
    (rows.iter())
        .map(|row| (row.id.clone(), row.source, row.state.clone()))
        .collect()
}

#[gpui::test]
fn the_list_shows_source_and_skipped(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = host(dir.path(), cx);
    let (workbench, mut cx) = workbench(&root, cx);
    // Not active, so the window doesn't ask by itself.
    cx.deactivate_window();

    // Untrusted: the folder's extension is listed, skipped, after the
    // user's.
    let rows = shown(&workbench, &mut cx);
    assert_eq!(
        summary(&rows),
        [
            (
                "acme.zig".into(),
                ExtensionSource::User,
                ExtensionState::Active
            ),
            (
                "team.odin".into(),
                ExtensionSource::Workspace,
                ExtensionState::SkippedUntrusted
            ),
        ]
    );
    let odin = &rows[1];
    assert_eq!(odin.state.label(), "Skipped (untrusted)");
    assert_eq!(
        (odin.name.as_str(), odin.kinds.as_str()),
        ("Odin", "1 lsp server")
    );
    let reason = format!("{} isn't trusted", root.display());
    assert!(odin.detail().contains("Workspace"), "{}", odin.detail());
    assert!(odin.detail().ends_with(&reason), "{}", odin.detail());

    // Trusted, it loads, with source Workspace.
    cx.update(|_, cx| trust::decide(&root, true, cx));
    cx.run_until_parked();
    let rows = shown(&workbench, &mut cx);
    let rows: Vec<_> = summary(&rows)
        .into_iter()
        .map(|(id, source, _)| (id, source))
        .collect();
    assert_eq!(
        rows,
        [
            ("team.odin".into(), ExtensionSource::Workspace),
            ("acme.zig".into(), ExtensionSource::User),
        ],
        "by name, A–Z"
    );
}

#[gpui::test]
fn a_skipped_row_with_a_users_id_is_its_own_row(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("project/.fastedit/extensions");
    install(&workspace, "zig", "acme.zig", "Zig");
    let root = host(dir.path(), cx);
    let (workbench, mut cx) = workbench(&root, cx);
    cx.deactivate_window();
    // The test platform answers prompts (`pending_prompt`).
    cx.update(|_, cx| cx.reset_prompt_builder());
    let user_zig = canonical(&dir.path().join("extensions/zig"));
    let server = ExtensionServer {
        extension_id: "acme.zig".into(),
        extension_name: "Zig".into(),
        extension_version: "1.2.3".into(),
        extension_license: "MIT".into(),
        extension_dir: user_zig.clone(),
        spec: ServerSpec::new("zls", "/opt/zls/bin/zls"),
    };
    cx.update(|_, cx| set_extension_servers(cx, vec![server]));
    cx.run_until_parked();

    let rows = shown(&workbench, &mut cx);
    assert_eq!(
        summary(&rows),
        [
            (
                "acme.zig".into(),
                ExtensionSource::User,
                ExtensionState::NeedsConsent {
                    servers: vec!["zls".into()]
                }
            ),
            (
                "team.odin".into(),
                ExtensionSource::Workspace,
                ExtensionState::SkippedUntrusted
            ),
            (
                "acme.zig".into(),
                ExtensionSource::Workspace,
                ExtensionState::SkippedUntrusted
            ),
        ]
    );
    let picker: Entity<Picker<ExtensionsPicker>> =
        workbench.read_with(&cx, |workbench, _| workbench.extensions_picker().clone());
    let ids: Vec<_> = picker.read_with(&cx, |picker, _| {
        (0..3).map(|ix| picker.delegate.match_id(ix)).collect()
    });
    // The row's dir is the manifest's, as the host was given it: not canonical
    // (macOS `/var` -> `/private/var`), unlike the server's `extension_dir`.
    assert_eq!(
        ids[0].as_ref(),
        dir.path().join("extensions/zig").to_string_lossy()
    );
    assert_ne!(ids[0], ids[2], "each row its own identity");

    // Clicking the skipped one asks nothing: it isn't the user's.
    let click = |ix: usize, cx: &mut VisualTestContext| {
        let row = picker.read_with(cx, |picker, _| picker.row_bounds(ix));
        cx.simulate_click(row.expect("laid out").center(), Modifiers::none());
        draw(cx);
    };
    click(2, &mut cx);
    assert!(!cx.has_pending_prompt());

    // Clicking the user's asks about its server.
    let _ = shown(&workbench, &mut cx);
    click(0, &mut cx);
    let (question, _) = cx.pending_prompt().expect("a prompt");
    assert_eq!(question, "Allow Zig to run a language server?");
}
