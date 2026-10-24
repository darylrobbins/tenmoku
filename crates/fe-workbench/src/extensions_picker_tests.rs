//! Extensions: Show Installed (`workbench.show_extensions`): the loaded
//! extensions with their kinds, licence, source and state; Enter on one
//! waiting on consent shows its prompt, and on any other its details.

use std::{fs, path::PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_lsp::ServerSpec;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{ApiVersion, LicenseClass, extension::MANIFEST_FILE},
    platform::config::batch_channel,
};
use serde_json::json;
use tempfile::TempDir;

use super::{ExtensionSource, ExtensionState, ExtensionsPicker};
use crate::{
    ExtensionServer, Workbench, pending_extension_consents, picker::Picker, set_extension_servers,
    tests::open_root,
};

fn manifest(id: &str, name: &str, license: &str, contributes: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"{name}\"\nversion = \"1.2.3\"\nlicense = \"{license}\"\n\n\
         [engine]\nfastedit = \"1.0\"\n\n[contributes]\n{contributes}"
    )
}

const NIGHT: &str = "name = \"Night\"\nappearance = \"dark\"\n";

/// An extensions directory with Zig (a theme and an lsp server) and Wasm
/// (grammars), kiln started over it.
fn host(cx: &mut TestAppContext) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let zig = dir.path().join("extensions/zig");
    fs::create_dir_all(zig.join("themes")).unwrap();
    fs::write(zig.join("themes/night.toml"), NIGHT).unwrap();
    let lsp = "themes = [\"themes/night.toml\"]\n\n[contributes.lsp.zls]\ncommand = \"zls\"\n";
    fs::write(
        zig.join(MANIFEST_FILE),
        manifest("acme.zig", "Zig", "MIT", lsp),
    )
    .unwrap();
    let wasm = dir.path().join("extensions/wasm");
    fs::create_dir_all(&wasm).unwrap();
    let grammars =
        "\n[[contributes.grammars]]\nlanguage = \"wat\"\nwasm = \"tree-sitter-wat.wasm\"\n";
    fs::write(
        wasm.join(MANIFEST_FILE),
        manifest("acme.wasm", "Wasm", "GPL-3.0-only", grammars),
    )
    .unwrap();
    let (_sender, batches) = batch_channel();
    let api = ApiVersion { major: 1, minor: 0 };
    let config = Config::new("dev.fastedit.test", "fastedit")
        .extensions(dir.path().join("extensions"), "fastedit", api)
        .extension_kind("lsp", json!({ "type": "object" }))
        .extension_kind("grammars", json!({ "type": "array" }))
        // Not the real watcher, whose thread the test scheduler can't follow.
        .watch_with(batches);
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config), []);
        // Where there are no native alerts, `kiln::init` sets up kiln-ui's
        // in-window prompt; the test platform answers prompts
        // (`pending_prompt`), not that one.
        cx.reset_prompt_builder();
    });
    cx.run_until_parked();
    dir
}

fn server(dir: &std::path::Path) -> ExtensionServer {
    ExtensionServer {
        extension_id: "acme.zig".into(),
        extension_name: "Zig".into(),
        extension_version: "1.2.3".into(),
        extension_license: "MIT".into(),
        extension_dir: dir.join("extensions/zig").canonicalize().unwrap(),
        spec: ServerSpec {
            args: vec!["--stdio".into()],
            languages: vec!["zig".into()],
            ..ServerSpec::new("zls", "/opt/zls/bin/zls")
        },
    }
}

pub(super) fn workbench(
    host: &TempDir,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    let root = host.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx.clone())
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(super) fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

pub(super) fn picker(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<Picker<ExtensionsPicker>> {
    workbench.read_with(cx, |workbench, _| workbench.extensions_picker().clone())
}

pub(super) fn rows(
    picker: &Entity<Picker<ExtensionsPicker>>,
    cx: &mut VisualTestContext,
) -> Vec<super::Row> {
    picker.read_with(cx, |picker, _| picker.delegate.rows())
}

#[gpui::test]
fn the_command_opens_the_picker(cx: &mut TestAppContext) {
    let host = host(cx);
    let (workbench, mut cx) = workbench(&host, cx);
    assert!(!workbench.read_with(&cx, |w, _| w.extensions_picker_open()));
    dispatch("workbench.show_extensions", &mut cx);
    assert!(workbench.read_with(&cx, |w, _| w.extensions_picker_open()));
}

#[gpui::test]
fn rows_show_name_kinds_licence_source_and_state(cx: &mut TestAppContext) {
    let host = host(cx);
    let (workbench, mut cx) = workbench(&host, cx);
    let picker = picker(&workbench, &mut cx);
    dispatch("workbench.show_extensions", &mut cx);
    let rows = rows(&picker, &mut cx);
    assert_eq!(rows.len(), 2);
    let summary = |row: &super::Row| {
        (
            row.name.clone(),
            row.id.clone(),
            row.version.clone(),
            row.kinds.clone(),
            row.license.clone(),
            row.class,
            row.source,
            row.state.clone(),
        )
    };
    assert_eq!(
        summary(&rows[0]),
        (
            "Wasm".into(),
            "acme.wasm".into(),
            "1.2.3".into(),
            "1 grammar".into(),
            "GPL-3.0-only".into(),
            LicenseClass::Copyleft,
            ExtensionSource::User,
            ExtensionState::Active,
        )
    );
    assert_eq!(
        summary(&rows[1]),
        (
            "Zig".into(),
            "acme.zig".into(),
            "1.2.3".into(),
            "1 theme, 1 lsp server".into(),
            "MIT".into(),
            LicenseClass::Permissive,
            ExtensionSource::User,
            ExtensionState::Active,
        )
    );
}

#[gpui::test]
fn an_extension_waiting_on_consent_shows_it(cx: &mut TestAppContext) {
    let host = host(cx);
    let (workbench, mut cx) = workbench(&host, cx);
    // Not active, so the window doesn't ask by itself.
    cx.deactivate_window();
    cx.update(|_, cx| set_extension_servers(cx, vec![server(host.path())]));
    cx.run_until_parked();
    let picker = picker(&workbench, &mut cx);
    dispatch("workbench.show_extensions", &mut cx);
    let zig = rows(&picker, &mut cx).remove(1);
    assert_eq!(
        zig.state,
        ExtensionState::NeedsConsent {
            servers: vec!["zls".into()]
        }
    );
    assert!(
        zig.detail().contains("/opt/zls/bin/zls --stdio"),
        "{}",
        zig.detail()
    );
    assert!(zig.detail().contains("pending"), "{}", zig.detail());

    // Declined.
    let pending = cx.update(|_, cx| pending_extension_consents(cx)).remove(0);
    cx.update(|_, cx| crate::decline_extension_server(cx, &pending).unwrap());
    dispatch("workbench.show_extensions", &mut cx);
    let zig = rows(&picker, &mut cx).remove(1);
    assert_eq!(
        zig.state,
        ExtensionState::Declined {
            servers: vec!["zls".into()]
        }
    );
    assert!(zig.detail().contains("declined"), "{}", zig.detail());
}

#[gpui::test]
fn enter_on_a_waiting_row_opens_its_consent_prompt(cx: &mut TestAppContext) {
    let host = host(cx);
    let (workbench, mut cx) = workbench(&host, cx);
    cx.deactivate_window();
    cx.update(|_, cx| set_extension_servers(cx, vec![server(host.path())]));
    cx.run_until_parked();
    let _ = &workbench;

    // Active: Enter shows its details.
    dispatch("workbench.show_extensions", &mut cx);
    cx.simulate_input("wasm");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    let (question, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(question, "Wasm 1.2.3");
    assert!(
        detail.contains("Licence: GPL-3.0-only (copyleft)"),
        "{detail}"
    );
    cx.simulate_prompt_answer("OK");
    draw(&mut cx);

    // Waiting: it asks about that extension.
    dispatch("workbench.show_extensions", &mut cx);
    cx.simulate_input("zig");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    let (question, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(question, "Allow Zig to run a language server?");
    assert!(detail.contains("/opt/zls/bin/zls"), "{detail}");
    let _: PathBuf = host.path().into();
}
