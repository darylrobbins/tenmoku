//! Installing a folder for development: what's linked and what's refused,
//! and a linked plugin rebuilt: the tree watcher sees the change behind
//! the link, and kiln's reload of it reloads the plugin, asking only when
//! it asks for more. The plugin is kiln-plugin's commands fixture.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use fe_command::{
    ContextFrame, ContextStack, Invocation,
    capability::{Capability, Grants},
};
use fe_gpui::commands::RunCommand;
use gpui::{App, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{ApiVersion, TreeWatcher},
    platform::config::batch_channel,
};
use kiln_plugin::grants::GrantStore;

use super::{link, linked_from};
use crate::{
    GRANTS_FILE, InstallError, PluginAsk, PluginState, extension_install::check_api,
    pending_plugin_grants, plugin_host::PluginHost, plugin_kind, plugin_log, plugin_states,
    set_plugin_dirs, set_plugin_extensions, tests::open_root,
};

/// Registers `acme.tool.hello`, `acme.tool.spin` and `acme.tool.invoke`.
const COMMANDS: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/commands.wat");

const TOOL: &str = "acme.tool";

/// Writes plugin [`TOOL`] at `version` into `dir`, its component `wat`,
/// asking for `capabilities`.
fn write_plugin(dir: &Path, version: &str, wat: &str, capabilities: &[&str]) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("plugin.wasm"), wat).unwrap();
    let capabilities: Vec<String> = (capabilities.iter())
        .map(|capability| format!("{capability:?}"))
        .collect();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"{TOOL}\"\nname = \"Tool\"\nversion = \"{version}\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [{}]\n",
            capabilities.join(", ")
        ),
    )
    .unwrap();
}

/// Removes the symlink `at`: a directory symlink is a directory on Windows.
fn remove_link(at: &Path) {
    fs::remove_file(at).or_else(|_| fs::remove_dir(at)).unwrap();
}

fn refusal(result: Result<crate::Installed, InstallError>) -> String {
    match result {
        Err(InstallError::Refused(reason)) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_linked_folder_installs_as_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let extensions = dir.path().join("extensions");
    let folder = dir.path().join("work/tool");
    write_plugin(&folder, "1.0.0", COMMANDS, &["commands"]);
    let api = check_api(Vec::new());

    let installed = link(&folder, &extensions, &api).unwrap();
    let canonical = kiln::core::platform::canonicalize(&folder).unwrap();
    let at = kiln::core::platform::canonicalize(&extensions)
        .unwrap()
        .join(TOOL);
    assert_eq!(installed.dir, at);
    assert!(installed.plugin);
    assert!(fs::symlink_metadata(&at).unwrap().file_type().is_symlink());
    assert_eq!(linked_from(&at), Some(canonical.clone()));
    // Linked again, the link is kept.
    assert_eq!(link(&folder, &extensions, &api).unwrap(), installed);
    assert_eq!(fs::read_link(&at).unwrap(), canonical);

    // `<id>` taken by anything but that link refuses it, leaving it be.
    remove_link(&at);
    let other = dir.path().join("work/other");
    fs::create_dir_all(&other).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&other, &at).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&other, &at).unwrap();
    let reason = refusal(link(&folder, &extensions, &api));
    assert!(reason.contains("isn't a link to this folder"), "{reason}");
    assert_eq!(fs::read_link(&at).unwrap(), other);
    remove_link(&at);
    fs::create_dir(&at).unwrap();
    refusal(link(&folder, &extensions, &api));
    assert!(fs::symlink_metadata(&at).unwrap().is_dir());
    fs::remove_dir(&at).unwrap();

    // So does its id installed in another directory.
    write_plugin(&extensions.join("tool"), "1.0.0", COMMANDS, &["commands"]);
    let reason = refusal(link(&folder, &extensions, &api));
    assert!(reason.contains("installed already"), "{reason}");
    fs::remove_dir_all(extensions.join("tool")).unwrap();

    // A folder with no manifest, a bad one, or the extensions directory
    // itself isn't linked.
    let empty = dir.path().join("work/empty");
    fs::create_dir_all(&empty).unwrap();
    refusal(link(&empty, &extensions, &api));
    fs::write(empty.join("extension.toml"), "id = \"acme.bad\"\n").unwrap();
    refusal(link(&empty, &extensions, &api));
    refusal(link(&extensions, &extensions, &api));
    refusal(link(dir.path(), &extensions, &api));
    assert_eq!(fs::read_dir(&extensions).unwrap().count(), 0);
}

/// Waits up to 1.5 s for a batch from `batches` naming `root`.
fn reported(batches: &mpsc::Receiver<Vec<PathBuf>>, root: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_millis(1500);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match batches.recv_timeout(left) {
            Ok(batch) if batch.iter().any(|path| path == root) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
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

/// The plugins' inits and shutdowns, in order.
fn lifecycle(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        (cx.global::<PluginHost>().record.lock().unwrap().iter())
            .filter(|line| line.starts_with("init ") || line.starts_with("shutdown "))
            .cloned()
            .collect()
    })
}

/// The messages of [`TOOL`]'s log.
fn logged(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        (plugin_log::lines(TOOL, cx).into_iter())
            .map(|line| line.message)
            .collect()
    })
}

#[gpui::test]
fn rebuilding_a_linked_plugin_reloads_it(cx: &mut TestAppContext) {
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let extensions = dir.path().join("extensions");
    fs::create_dir_all(&extensions).unwrap();
    let folder = dir.path().join("work/tool");
    write_plugin(&folder, "1.0.0", COMMANDS, &["commands"]);

    // The tree watcher, linked after it starts, sees a rebuild behind the
    // link as a change under the extensions directory.
    let (sink, batches) = mpsc::channel();
    let watcher = TreeWatcher::new(vec![extensions.clone()], move |batch| {
        sink.send(batch).ok();
    })
    .unwrap();
    assert!(reported(&batches, &extensions), "the first batch");
    link(&folder, &extensions, &check_api(Vec::new())).unwrap();
    assert!(reported(&batches, &extensions), "the link");
    let rebuilt = format!("{COMMANDS}\n;; rebuilt\n");
    fs::write(folder.join("plugin.wasm"), &rebuilt).unwrap();
    assert!(
        reported(&batches, &extensions),
        "the rebuild behind the link"
    );
    drop(watcher);

    // kiln, its batches the test's, hands the host the plugin.
    let state = dir.path().join("state");
    GrantStore::new(state.join(GRANTS_FILE))
        .grant(TOOL, (1, 0, 0), &grants(&["commands"]), &[], false)
        .unwrap();
    let (sender, batches) = batch_channel();
    let plugin = plugin_kind();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .extensions(
            extensions.clone(),
            &fe_config::paths::app_id().short,
            ApiVersion { major: 1, minor: 1 },
        )
        .extension_kind(plugin.name, plugin.schema)
        .extension_files(|_| vec![PathBuf::from("plugin.wasm")])
        .on_extensions(|set, cx| set_plugin_extensions(cx, set))
        .watch_with(batches);
    let cache = dir.path().join("cache");
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config), []);
        cx.reset_prompt_builder();
        set_plugin_dirs(cx, state, cache);
        crate::plugin_host::set_preview(cx, true);
    });
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let (workbench, vcx) = open_root(cx, &root);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    let mut vcx = vcx.clone();
    let running = |cx: &App| {
        (plugin_states(cx).iter()).any(|info| info.id == TOOL && info.state == PluginState::Running)
    };
    until(&mut vcx, "the linked plugin to run", running);
    assert_eq!(logged(&mut vcx), ["loaded 1.0.0"]);

    // Show Installed lists it as linked from the folder.
    let run = RunCommand(
        Invocation::of("workbench.show_extensions")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    vcx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(&mut vcx);
    let picker = workbench.read_with(&vcx, |workbench, _| workbench.extensions_picker().clone());
    let detail = picker.read_with(&vcx, |picker, _| {
        let rows = picker.delegate.rows();
        rows.iter().find(|row| row.id == TOOL).unwrap().detail()
    });
    let canonical = kiln::core::platform::canonicalize(&folder).unwrap();
    let source = format!("Linked from {}", canonical.display());
    assert!(detail.contains(&source), "{detail}");
    vcx.simulate_keystrokes("escape");

    // Rebuilt with the same capabilities, it reloads, asking nothing.
    fs::write(folder.join("plugin.wasm"), format!("{rebuilt};; again\n")).unwrap();
    sender.send(vec![extensions.clone()]);
    until(&mut vcx, "the rebuild to reload", |cx| {
        plugin_log::lines(TOOL, cx).len() == 2
    });
    assert_eq!(logged(&mut vcx), ["loaded 1.0.0", "reloaded 1.0.0"]);
    until(&mut vcx, "the reload to run", running);
    draw(&mut vcx);
    assert_eq!(
        lifecycle(&mut vcx),
        [
            format!("init {TOOL}"),
            format!("shutdown {TOOL}"),
            format!("init {TOOL}")
        ]
    );
    assert!(!vcx.has_pending_prompt());
    assert_eq!(vcx.update(|_, cx| pending_plugin_grants(cx)), []);

    // Rebuilt asking for more, it asks, the old version running meanwhile.
    write_plugin(&folder, "1.1.0", COMMANDS, &["commands", "status"]);
    sender.send(vec![extensions.clone()]);
    until(&mut vcx, "the escalation to wait", |cx| {
        !pending_plugin_grants(cx).is_empty()
    });
    draw(&mut vcx);
    let (question, _) = vcx.pending_prompt().expect("the escalation's prompt");
    assert_eq!(question, "Allow Tool 1.1.0 more access?");
    assert_eq!(
        vcx.update(|_, cx| pending_plugin_grants(cx))[0].ask,
        PluginAsk::Escalation {
            added: grants(&["status"]),
            keep_running: Some((1, 0, 0)),
        }
    );
    assert!(vcx.update(|_, cx| running(cx)));
    assert_eq!(lifecycle(&mut vcx).len(), 3);
}

fn grants(capabilities: &[&str]) -> Grants {
    (capabilities.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}
