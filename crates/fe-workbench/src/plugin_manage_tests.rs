//! Plugins in Extensions: Show Installed, and removing them: each row's
//! capabilities, state and full-access marker, or that it needs the
//! preview; Revoke Plugin Grants; and Uninstall Plugin, its confirmation,
//! what confirming removes, what cancelling keeps, and what a keychain that
//! can't delete keeps. The plugin is kiln-plugin's empty fixture, found by
//! kiln in a test extensions directory.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use fe_command::{
    ContextFrame, ContextStack, Invocation,
    capability::{Capability, Grants},
};
use fe_gpui::commands::RunCommand;
use gpui::{App, Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{
        ApiVersion,
        secrets::{Secret, SecretError, SecretStore},
    },
    platform::config::batch_channel,
};
use kiln_plugin::{
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
    secrets::PluginSecrets,
};
use tempfile::TempDir;

use super::{PluginRow, set_stores};
use crate::{
    ExtensionServer, GRANTS_FILE, PluginAnswer, PluginDiagnostic, PluginInfo, PluginState,
    SECRETS_INDEX_FILE, StopReason, Workbench,
    extensions_picker::ExtensionState,
    pending_plugin_grants,
    plugin_host::{Found, PluginHost, prompt_finished, set_plugins, set_preview},
    plugin_kind, plugin_states, reevaluate, set_extension_servers, set_plugin_dirs,
    set_plugin_extensions,
    tests::open_root,
    unload_plugin,
};

/// Exports `init` and `shutdown`, and does nothing.
const EMPTY: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/empty.wat");

const ID: &str = "acme.tool";

fn grants(capabilities: &[&str]) -> Grants {
    (capabilities.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn info(granted: &[&str], state: PluginState) -> PluginInfo {
    PluginInfo {
        id: ID.into(),
        name: "Tool".into(),
        version: (1, 0, 0),
        dir: PathBuf::from("/ext/tool"),
        requested: grants(&["buffer.read", "net:*"]),
        granted: grants(granted),
        state,
    }
}

#[test]
fn rows_show_capabilities_state_and_a_full_access_marker() {
    let full = PluginRow::of(&info(&["buffer.read", "net:*"], PluginState::Running));
    assert_eq!(full.state, ExtensionState::Running);
    assert!(full.full_access);
    let detail = full.detail().join(" · ");
    assert!(detail.starts_with("Full access · "), "{detail}");
    assert!(
        detail.contains("Asks to: buffer.read: read the text of your open files; net:*: connect to any host on the internet"),
        "{detail}"
    );
    let scoped = PluginRow::of(&info(&["net:api.example.com"], PluginState::Running));
    assert!(!scoped.full_access);
    let detail = scoped.detail().join(" · ");
    assert!(!detail.contains("Full access"), "{detail}");
    assert!(
        detail.contains("Granted: net:api.example.com: connect to api.example.com"),
        "{detail}"
    );

    let stopped = PluginState::Stopped(PluginDiagnostic {
        reason: StopReason::Trap,
        message: "wasm trap: unreachable".into(),
    });
    for (state, shown, reason) in [
        (PluginState::WaitingForGrant, "Waiting for grant", None),
        (PluginState::Declined, "Declined", None),
        (
            stopped,
            "Stopped",
            Some("Stopped: trapped: wasm trap: unreachable"),
        ),
    ] {
        let row = PluginRow::of(&info(&[], state));
        assert_eq!(row.state.label(), shown);
        assert_eq!(row.reason.as_deref(), reason);
        assert!(!row.full_access);
    }
}

/// Plugin `id`, named `name`, in `dir`, asking for `buffer.read`, with the
/// language server `tls` too if `lsp`.
fn write_plugin(dir: &Path, id: &str, name: &str, lsp: bool) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("plugin.wasm"), EMPTY).unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"{id}\"\nname = \"{name}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [\"buffer.read\"]\n{}",
            if lsp {
                "\n[contributes.lsp.tls]\ncommand = \"tls\"\n"
            } else {
                ""
            }
        ),
    )
    .unwrap();
}

/// A keychain in memory that can be made unavailable.
#[derive(Default)]
struct Keychain {
    entries: Mutex<BTreeMap<(String, String), String>>,
    unavailable: AtomicBool,
}

impl Keychain {
    fn check(&self) -> Result<(), SecretError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(SecretError::Unavailable(
                "the test's keychain is off".into(),
            ));
        }
        Ok(())
    }

    fn count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

impl SecretStore for Keychain {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        self.check()?;
        let entries = self.entries.lock().unwrap();
        let key = (service.to_owned(), account.to_owned());
        Ok(entries.get(&key).map(Secret::new))
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.check()?;
        let key = (service.to_owned(), account.to_owned());
        self.entries.lock().unwrap().insert(key, secret.to_owned());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.check()?;
        let key = (service.to_owned(), account.to_owned());
        self.entries.lock().unwrap().remove(&key);
        Ok(())
    }
}

/// A test's plugin, its directories and its keychain.
struct Fixture {
    dir: TempDir,
    /// Where kiln finds the plugin: a symlink, if the test asked for one.
    plugin_dir: PathBuf,
    keychain: Arc<Keychain>,
}

impl Fixture {
    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn store(&self) -> GrantStore {
        GrantStore::new(self.state().join(GRANTS_FILE))
    }

    fn secrets(&self) -> PluginSecrets {
        let service = fe_config::paths::app_id().reverse_dns;
        let index = self.state().join(SECRETS_INDEX_FILE);
        PluginSecrets::new(self.keychain.clone(), service, ID, index)
    }
}

/// Plugin [`ID`], granted what it asks for and holding a secret, beside a
/// theme-only extension, kiln finding them with the preview `preview`: in
/// a symlinked directory if `linked`. Then a workbench, the plugin running
/// if the preview's on.
fn start(
    cx: &mut TestAppContext,
    preview: bool,
    linked: bool,
) -> (Fixture, Entity<Workbench>, VisualTestContext) {
    start_with(
        cx,
        Setup {
            preview,
            linked,
            ..Setup::default()
        },
    )
}

/// What [`start_with`] sets up beyond [`start`].
#[derive(Clone, Copy, Default)]
struct Setup {
    preview: bool,
    linked: bool,
    /// The link is a junction, if `linked`.
    junction: bool,
    /// The plugin's extension contributes a language server too.
    lsp: bool,
    /// [`OTHER`] is installed and granted too.
    other: bool,
    /// The plugin host isn't handed the extensions: it hasn't found them.
    unhosted: bool,
}

/// A second plugin's id.
const OTHER: &str = "acme.other";

/// [`start`], as `setup` says.
fn start_with(
    cx: &mut TestAppContext,
    setup: Setup,
) -> (Fixture, Entity<Workbench>, VisualTestContext) {
    let Setup {
        preview,
        linked,
        junction,
        lsp,
        other,
        unhosted,
    } = setup;
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let extensions = dir.path().join("extensions");
    let plugin_dir = extensions.join("tool");
    if linked {
        let target = dir.path().join("elsewhere").join("tool");
        write_plugin(&target, ID, "Tool", lsp);
        fs::create_dir_all(&extensions).unwrap();
        if junction {
            crate::plugin_dev::make_junction(&target, &plugin_dir).unwrap();
        } else {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &plugin_dir).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&target, &plugin_dir).unwrap();
        }
    } else {
        write_plugin(&plugin_dir, ID, "Tool", lsp);
    }
    if other {
        write_plugin(&extensions.join("other"), OTHER, "Other", false);
    }
    let night = extensions.join("night");
    fs::create_dir_all(&night).unwrap();
    fs::write(
        night.join("extension.toml"),
        "id = \"acme.night\"\nname = \"Night\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\ntenmoku = \"1.1\"\n",
    )
    .unwrap();
    let fixture = Fixture {
        dir,
        plugin_dir,
        keychain: Arc::default(),
    };
    (fixture.store())
        .grant(ID, (1, 0, 0), &grants(&["buffer.read"]), &[], false)
        .unwrap();
    if other {
        (fixture.store())
            .grant(OTHER, (1, 0, 0), &grants(&["buffer.read"]), &[], false)
            .unwrap();
    }
    fixture.secrets().set("token", "s3cret").unwrap();

    let (_sender, batches) = batch_channel();
    let plugin = plugin_kind();
    let mut config = Config::new("dev.fastedit.test", "fastedit")
        .extensions(
            extensions,
            &fe_config::paths::app_id().short,
            ApiVersion { major: 1, minor: 1 },
        )
        .extension_kind(plugin.name, plugin.schema)
        .extension_kind("lsp", serde_json::json!({ "type": "object" }))
        // Not the real watcher, whose thread the test scheduler can't follow.
        .watch_with(batches);
    if !unhosted {
        config = config.on_extensions(|set, cx| set_plugin_extensions(cx, set));
    }
    let state = fixture.state();
    let cache = fixture.dir.path().join("cache");
    let keychain: Arc<dyn SecretStore> = fixture.keychain.clone();
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config), []);
        // The test platform answers prompts (`pending_prompt`), not
        // kiln-ui's in-window one.
        cx.reset_prompt_builder();
        set_stores(cx, state.clone(), keychain);
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, preview);
    });
    let root = fixture.dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let (workbench, vcx) = open_root(cx, &root);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    let mut vcx = vcx.clone();
    if preview && !unhosted {
        until(&mut vcx, "the plugins to run", |cx| {
            (plugin_states(cx).iter()).all(|info| info.state == PluginState::Running)
                && state_of(cx).is_some()
        });
    }
    draw(&mut vcx);
    (fixture, workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// Runs until `done`: plugins load and stop on their own threads.
fn until(cx: &mut VisualTestContext, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if cx.update(|_, cx| done(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn state_of(cx: &App) -> Option<PluginState> {
    (plugin_states(cx).into_iter())
        .find(|info| info.id == ID)
        .map(|info| info.state)
}

fn unloaded(cx: &App) -> bool {
    matches!(
        state_of(cx),
        Some(PluginState::Stopped(PluginDiagnostic {
            reason: StopReason::Unloaded,
            ..
        }))
    )
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The extensions picker's rows: each one's ID, label and detail.
fn rows(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(String, String, String)> {
    let picker = workbench.read_with(cx, |workbench, _| workbench.extensions_picker().clone());
    picker.read_with(cx, |picker, _| {
        (picker.delegate.rows().into_iter())
            .map(|row| (row.id.clone(), row.label(), row.detail()))
            .collect()
    })
}

/// The IDs of the rows `command`'s picker lists.
fn listed(command: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    dispatch(command, cx);
    (rows(workbench, cx).into_iter())
        .map(|(id, ..)| id)
        .collect()
}

/// Runs `command`, whose picker lists the plugins alone, and picks [`ID`].
fn pick_plugin(command: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let shown = listed(command, workbench, cx);
    assert!(shown.iter().any(|id| id == ID), "{shown:?}");
    assert!(!shown.iter().any(|id| id == "acme.night"), "{shown:?}");
    cx.simulate_input("tool");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                format!(
                    "{} {}",
                    toast.message,
                    toast.detail.as_deref().unwrap_or("")
                )
            })
            .collect()
    })
}

#[gpui::test]
fn with_the_preview_off_a_plugin_is_listed_as_needing_it(cx: &mut TestAppContext) {
    let (_fixture, workbench, mut cx) = start(cx, false, false);
    dispatch("workbench.show_extensions", &mut cx);
    let shown = rows(&workbench, &mut cx);
    let (_, label, detail) = (shown.iter())
        .find(|(id, ..)| id == ID)
        .expect("the plugin is listed");
    assert_eq!(label, "Needs plugins preview");
    assert!(detail.contains("extensions.plugins_preview"), "{detail}");
    let (_, label, _) = shown.iter().find(|(id, ..)| id == "acme.night").unwrap();
    assert_eq!(label, "Active");
}

#[gpui::test]
fn the_uninstall_confirmation_says_it_deletes_secrets(cx: &mut TestAppContext) {
    let (_fixture, workbench, mut cx) = start(cx, true, false);
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    let (question, detail) = cx.pending_prompt().expect("the confirmation");
    assert_eq!(question, "Uninstall the plugin Tool?");
    assert!(
        detail.contains("Its stored secrets are deleted"),
        "{detail}"
    );
    cx.simulate_prompt_answer("Cancel");
}

/// Uninstalls the plugin, kiln finding it through a symlink if `linked`.
fn uninstalls(cx: &mut TestAppContext, linked: bool, junction: bool) {
    let setup = Setup {
        preview: true,
        linked,
        junction,
        ..Setup::default()
    };
    let (fixture, workbench, mut cx) = start_with(cx, setup);
    let target = dunce::canonicalize(&fixture.plugin_dir).unwrap();
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    cx.simulate_prompt_answer("Uninstall");
    until(&mut cx, "the directory to go", |_| {
        fs::symlink_metadata(&fixture.plugin_dir).is_err()
    });
    draw(&mut cx);
    assert!(cx.update(|_, cx| unloaded(cx)));
    assert_eq!(fixture.keychain.count(), 0);
    assert_eq!(fixture.secrets().names().unwrap(), Vec::<String>::new());
    assert_eq!(fixture.store().get(ID).unwrap(), None);
    // Only the link goes.
    assert_eq!(target.join("extension.toml").exists(), linked);
    let shown = toasts(&workbench, &mut cx);
    assert!(
        shown
            .iter()
            .any(|toast| toast.starts_with("Uninstalled Tool")),
        "{shown:?}"
    );
}

#[gpui::test]
fn confirming_uninstall_removes_everything(cx: &mut TestAppContext) {
    uninstalls(cx, false, false);
}

#[gpui::test]
fn confirming_uninstall_removes_only_a_symlinks_link(cx: &mut TestAppContext) {
    uninstalls(cx, true, false);
}

#[gpui::test]
fn uninstall_removes_a_junction_link_only(cx: &mut TestAppContext) {
    uninstalls(cx, true, true);
}

#[gpui::test]
fn links_to_recognises_a_junction(cx: &mut TestAppContext) {
    let _ = cx;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    // Linked to the canonical folder, as installs link: macOS's temp
    // folder is under the /var symlink.
    let target = dunce::canonicalize(&target).unwrap();
    let at = dir.path().join("link");
    crate::plugin_dev::make_junction(&target, &at).unwrap();
    assert!(crate::plugin_dev::links_to(&at, &target));
    assert!(!crate::plugin_dev::links_to(
        &at,
        &dunce::canonicalize(&other).unwrap()
    ));
    assert!(!crate::plugin_dev::links_to(&target, &target));
    assert_eq!(crate::plugin_dev::linked_from(&at), Some(target));
}

#[gpui::test]
fn cancelling_uninstall_changes_nothing(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, true, false);
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    cx.simulate_prompt_answer("Cancel");
    draw(&mut cx);
    assert_eq!(cx.update(|_, cx| state_of(cx)), Some(PluginState::Running));
    assert_eq!(fixture.keychain.count(), 1);
    assert!(fixture.store().get(ID).unwrap().is_some());
    assert!(fixture.plugin_dir.join("extension.toml").exists());
}

#[gpui::test]
fn a_failed_secret_delete_keeps_the_plugin_installed(cx: &mut TestAppContext) {
    let setup = Setup {
        preview: true,
        other: true,
        ..Setup::default()
    };
    let (fixture, workbench, mut cx) = start_with(cx, setup);
    // Another plugin, stopped by hand, stays stopped.
    cx.update(|_, cx| unload_plugin(OTHER, cx));
    let other_stopped = |cx: &App| {
        (plugin_states(cx).into_iter()).any(|info| {
            info.id == OTHER
                && matches!(&info.state, PluginState::Stopped(d) if d.reason == StopReason::Unloaded)
        })
    };
    until(&mut cx, "the other to stop", other_stopped);
    fixture.keychain.unavailable.store(true, Ordering::SeqCst);
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    cx.simulate_prompt_answer("Uninstall");
    let deadline = Instant::now() + Duration::from_secs(30);
    let toast = loop {
        draw(&mut cx);
        let shown = toasts(&workbench, &mut cx);
        if let Some(toast) = shown
            .into_iter()
            .find(|toast| toast.contains("Can't uninstall"))
        {
            break toast;
        }
        assert!(Instant::now() < deadline, "no toast");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(toast.contains("no keychain is available"), "{toast}");
    assert!(toast.contains("stays installed"), "{toast}");
    // It runs again, its grants, secret and directory where they were.
    until(&mut cx, "the plugin to run again", |cx| {
        state_of(cx) == Some(PluginState::Running)
    });
    assert!(fixture.store().get(ID).unwrap().is_some());
    assert_eq!(fixture.keychain.count(), 1);
    assert!(fixture.plugin_dir.join("extension.toml").exists());
    assert!(cx.update(|_, cx| other_stopped(cx)));
}

#[gpui::test]
fn revoke_removes_grants_and_unloads(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, true, false);
    pick_plugin("workbench.revoke_plugin_grants", &workbench, &mut cx);
    until(&mut cx, "the plugin to stop", unloaded);
    until(&mut cx, "the grants to go", |_| {
        fixture.store().get(ID).unwrap().is_none()
    });
    assert!(fixture.plugin_dir.join("extension.toml").exists());
    // Its next load asks again.
    cx.update(|_, cx| reevaluate(cx));
    until(&mut cx, "it to wait for its grant", |cx| {
        state_of(cx) == Some(PluginState::WaitingForGrant)
    });
    let waiting = cx.update(|_, cx| pending_plugin_grants(cx));
    assert!(waiting.iter().any(|pending| pending.id == ID));
}

/// Revoked, the row shows nothing granted: no Full access, no grants.
#[gpui::test]
fn a_revoked_row_shows_no_grants(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, true, false);
    dispatch("workbench.show_extensions", &mut cx);
    let detail = |cx: &mut VisualTestContext| {
        let shown = rows(&workbench, cx);
        let (_, _, detail) = shown.into_iter().find(|(id, ..)| id == ID).unwrap();
        detail
    };
    assert!(detail(&mut cx).contains("Granted: buffer.read"));
    cx.simulate_keystrokes("escape");
    pick_plugin("workbench.revoke_plugin_grants", &workbench, &mut cx);
    until(&mut cx, "the grants to go", |_| {
        fixture.store().get(ID).unwrap().is_none()
    });
    draw(&mut cx);
    dispatch("workbench.show_extensions", &mut cx);
    let shown = detail(&mut cx);
    assert!(!shown.contains("Granted:"), "{shown}");
}

/// Waiting for its grant with its prompt showing: revoking stops the
/// prompt, whose answer then records nothing.
#[gpui::test]
fn revoking_a_plugin_being_asked_about_records_no_answer(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, false, false);
    fixture.store().forget(ID).unwrap();
    cx.update(|_, cx| set_preview(cx, true));
    until(&mut cx, "it to wait", |cx| {
        state_of(cx) == Some(PluginState::WaitingForGrant)
    });
    draw(&mut cx);
    let (question, _) = cx.pending_prompt().expect("the host asks");
    assert_eq!(question, "Allow the plugin Tool to load?");
    pick_plugin("workbench.revoke_plugin_grants", &workbench, &mut cx);
    until(&mut cx, "it to stop", unloaded);
    cx.simulate_prompt_answer("Allow");
    draw(&mut cx);
    assert_eq!(fixture.store().get(ID).unwrap(), None);
    assert!(cx.update(|_, cx| unloaded(cx)));
}

/// Enter on a plugin waiting, its prompt dismissed in this window, asks
/// again through the host's prompt, which records the answer.
#[gpui::test]
fn enter_on_a_waiting_plugin_asks_for_its_grant(cx: &mut TestAppContext) {
    let (fixture, _workbench, mut cx) = start(cx, false, false);
    fixture.store().forget(ID).unwrap();
    cx.update(|_, cx| set_preview(cx, true));
    until(&mut cx, "it to wait", |cx| {
        state_of(cx) == Some(PluginState::WaitingForGrant)
    });
    draw(&mut cx);
    let pending = cx.update(|_, cx| pending_plugin_grants(cx)).remove(0);
    cx.update(|window, cx| {
        prompt_finished(window.window_handle(), pending, PluginAnswer::Pending, cx);
    });
    // The platform's prompt, answered now, decides nothing.
    cx.simulate_prompt_answer("Allow");
    draw(&mut cx);
    assert!(!cx.has_pending_prompt());

    dispatch("workbench.show_extensions", &mut cx);
    cx.simulate_input("tool");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    let (question, _) = cx.pending_prompt().expect("its grant prompt");
    assert_eq!(question, "Allow the plugin Tool to load?");
    cx.simulate_prompt_answer("Allow");
    until(&mut cx, "it to run", |cx| {
        state_of(cx) == Some(PluginState::Running)
    });
    assert!(fixture.store().get(ID).unwrap().is_some());
}

/// Confirmed while its load runs, uninstalling deletes nothing until the
/// load has finished.
#[gpui::test]
fn uninstall_waits_for_a_load_under_way(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, false, false);
    let (gate, receiver) = async_channel::unbounded();
    cx.update(|_, cx| {
        cx.global_mut::<PluginHost>().load_gate = Some(receiver);
        set_preview(cx, true);
    });
    draw(&mut cx);
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    cx.simulate_prompt_answer("Uninstall");
    draw(&mut cx);
    assert_eq!(fixture.keychain.count(), 1, "nothing deleted mid-load");
    assert!(fixture.plugin_dir.exists());
    gate.send_blocking(()).unwrap();
    until(&mut cx, "the directory to go", |_| {
        !fixture.plugin_dir.exists()
    });
    assert_eq!(fixture.keychain.count(), 0);
    assert_eq!(fixture.store().get(ID).unwrap(), None);
    assert_ne!(cx.update(|_, cx| state_of(cx)), Some(PluginState::Running));
}

/// A disabled plugin, which the host doesn't run, is still listed to
/// uninstall, and uninstalls.
#[gpui::test]
fn a_disabled_plugin_can_be_uninstalled(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, true, false);
    cx.update(|_, cx| {
        kiln::extensions::set_disabled(cx, [ID]);
        set_plugins(cx, Vec::new());
    });
    until(&mut cx, "the host to drop it", |cx| {
        plugin_states(cx).is_empty()
    });
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    cx.simulate_prompt_answer("Uninstall");
    until(&mut cx, "the directory to go", |_| {
        !fixture.plugin_dir.exists()
    });
    assert_eq!(fixture.keychain.count(), 0);
    assert_eq!(fixture.store().get(ID).unwrap(), None);
}

/// Replaced by another extension while the confirmation shows, its
/// directory is left alone.
#[gpui::test]
fn a_replaced_directory_is_not_uninstalled(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, true, false);
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    let manifest = fixture.plugin_dir.join("extension.toml");
    let other = fs::read_to_string(&manifest)
        .unwrap()
        .replace(ID, "acme.other");
    fs::write(&manifest, other).unwrap();
    cx.simulate_prompt_answer("Uninstall");
    let deadline = Instant::now() + Duration::from_secs(30);
    let toast = loop {
        draw(&mut cx);
        let shown = toasts(&workbench, &mut cx);
        if let Some(toast) = shown.into_iter().find(|t| t.contains("Can't uninstall")) {
            break toast;
        }
        assert!(Instant::now() < deadline, "no toast");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(toast.contains("replaced"), "{toast}");
    assert!(manifest.exists());
    assert_eq!(fixture.keychain.count(), 1);
    assert!(fixture.store().get(ID).unwrap().is_some());
}

/// With its server waiting on consent and the preview off, the row says
/// both.
#[gpui::test]
fn a_plugin_with_a_language_server_shows_both_states(cx: &mut TestAppContext) {
    let setup = Setup {
        lsp: true,
        ..Setup::default()
    };
    let (fixture, workbench, mut cx) = start_with(cx, setup);
    let server = ExtensionServer {
        extension_id: ID.into(),
        extension_name: "Tool".into(),
        extension_version: "1.0.0".into(),
        extension_license: "MIT".into(),
        extension_dir: dunce::canonicalize(&fixture.plugin_dir).unwrap(),
        spec: fe_lsp::ServerSpec {
            languages: vec!["zig".into()],
            ..fe_lsp::ServerSpec::new("tls", "/opt/tls/bin/tls")
        },
    };
    cx.deactivate_window();
    cx.update(|_, cx| set_extension_servers(cx, vec![server]));
    draw(&mut cx);
    dispatch("workbench.show_extensions", &mut cx);
    let shown = rows(&workbench, &mut cx);
    let (_, label, detail) = shown.into_iter().find(|(id, ..)| id == ID).unwrap();
    assert_eq!(label, "Needs consent · Plugin: Needs plugins preview");
    assert!(detail.contains("extensions.plugins_preview"), "{detail}");
}

/// Gives the plugin threads time: what a load would start, it has.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..20 {
        draw(cx);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// An evaluation that decided before `command` (revoke or uninstall) ran
/// and applies after it finished decides nothing of the plugin: it doesn't
/// load, and shows no grants.
fn a_stale_evaluation_after(command: &str, cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, false, false);
    let (gate, receiver) = async_channel::unbounded();
    cx.update(|_, cx| {
        cx.global_mut::<PluginHost>().evaluation_gate = Some(receiver);
        set_preview(cx, true);
    });
    draw(&mut cx);
    pick_plugin(command, &workbench, &mut cx);
    if command == "workbench.uninstall_plugin" {
        cx.simulate_prompt_answer("Uninstall");
    }
    until(&mut cx, "the grants to go", |_| {
        fixture.store().get(ID).unwrap().is_none()
    });
    cx.update(|_, cx| cx.global_mut::<PluginHost>().evaluation_gate = None);
    gate.send_blocking(()).unwrap();
    settle(&mut cx);
    assert_ne!(cx.update(|_, cx| state_of(cx)), Some(PluginState::Running));
    let granted = cx.update(|_, cx| plugin_states(cx));
    assert!(
        granted.iter().all(|info| info.granted.is_empty()),
        "{granted:?}"
    );
}

#[gpui::test]
fn a_stale_evaluation_after_revoke_decides_nothing(cx: &mut TestAppContext) {
    a_stale_evaluation_after("workbench.revoke_plugin_grants", cx);
}

#[gpui::test]
fn a_stale_evaluation_after_uninstall_decides_nothing(cx: &mut TestAppContext) {
    a_stale_evaluation_after("workbench.uninstall_plugin", cx);
}

/// While an uninstall waits for a load, a revoke of the same plugin is
/// refused, and nothing loads it or records its grants until the uninstall
/// is done.
#[gpui::test]
fn one_operation_at_a_time(cx: &mut TestAppContext) {
    let (fixture, workbench, mut cx) = start(cx, false, false);
    let (gate, receiver) = async_channel::unbounded();
    cx.update(|_, cx| {
        cx.global_mut::<PluginHost>().load_gate = Some(receiver);
        set_preview(cx, true);
    });
    draw(&mut cx);
    pick_plugin("workbench.uninstall_plugin", &workbench, &mut cx);
    cx.simulate_prompt_answer("Uninstall");
    draw(&mut cx);
    pick_plugin("workbench.revoke_plugin_grants", &workbench, &mut cx);
    let shown = toasts(&workbench, &mut cx);
    assert!(
        shown
            .iter()
            .any(|toast| toast.starts_with("Can't revoke Tool's grants")
                && toast.contains("already")),
        "{shown:?}"
    );
    assert!(fixture.store().get(ID).unwrap().is_some(), "not yet");
    cx.update(|_, cx| reevaluate(cx));
    draw(&mut cx);
    cx.update(|_, cx| cx.global_mut::<PluginHost>().load_gate = None);
    gate.send_blocking(()).unwrap();
    until(&mut cx, "the directory to go", |_| {
        !fixture.plugin_dir.exists()
    });
    settle(&mut cx);
    assert_eq!(fixture.store().get(ID).unwrap(), None);
    assert_eq!(fixture.keychain.count(), 0);
    assert_ne!(cx.update(|_, cx| state_of(cx)), Some(PluginState::Running));
}

/// With the preview on and the host not yet deciding it, an installed
/// plugin is listed, starting, to revoke or uninstall.
#[gpui::test]
fn a_plugin_the_host_has_not_decided_is_listed(cx: &mut TestAppContext) {
    let setup = Setup {
        preview: true,
        unhosted: true,
        ..Setup::default()
    };
    let (_fixture, workbench, mut cx) = start_with(cx, setup);
    assert!(cx.update(|_, cx| plugin_states(cx).is_empty()));
    for command in [
        "workbench.revoke_plugin_grants",
        "workbench.uninstall_plugin",
    ] {
        assert_eq!(listed(command, &workbench, &mut cx), [ID], "{command}");
        cx.simulate_keystrokes("escape");
        draw(&mut cx);
    }
    dispatch("workbench.show_extensions", &mut cx);
    let shown = rows(&workbench, &mut cx);
    let (_, label, _) = shown.into_iter().find(|(id, ..)| id == ID).unwrap();
    assert_eq!(label, "Starting");
}

/// Revoked while the host's first evaluation of it is under way, before it
/// has a slot, it shows stopped once that evaluation is done.
#[gpui::test]
fn revoking_before_the_first_evaluation_leaves_it_stopped(cx: &mut TestAppContext) {
    let setup = Setup {
        preview: true,
        unhosted: true,
        ..Setup::default()
    };
    let (fixture, workbench, mut cx) = start_with(cx, setup);
    let (gate, receiver) = async_channel::unbounded();
    let found = found(&fixture.plugin_dir);
    cx.update(|_, cx| {
        cx.global_mut::<PluginHost>().evaluation_gate = Some(receiver);
        set_plugins(cx, vec![found]);
    });
    draw(&mut cx);
    assert!(
        cx.update(|_, cx| plugin_states(cx).is_empty()),
        "no slot yet"
    );
    pick_plugin("workbench.revoke_plugin_grants", &workbench, &mut cx);
    until(&mut cx, "the grants to go", |_| {
        fixture.store().get(ID).unwrap().is_none()
    });
    cx.update(|_, cx| cx.global_mut::<PluginHost>().evaluation_gate = None);
    gate.send_blocking(()).unwrap();
    settle(&mut cx);
    assert!(cx.update(|_, cx| unloaded(cx)));
    dispatch("workbench.show_extensions", &mut cx);
    let shown = rows(&workbench, &mut cx);
    let (_, label, _) = shown.into_iter().find(|(id, ..)| id == ID).unwrap();
    assert_eq!(label, "Stopped");
}

/// The plugin in extension `dir`, as the host is handed it.
fn found(dir: &Path) -> Found {
    let api = kiln::core::extension::ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = kiln::core::extension::load_manifest(dir, &api);
    let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    Found::new(&manifest, Ok(plugin))
}
