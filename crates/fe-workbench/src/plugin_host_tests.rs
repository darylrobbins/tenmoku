//! The plugin host with the preview on: a plugin waits for its grant, the
//! prompt records the answer, an update that asks for more keeps the old
//! version running, a granted plugin's commands are listed and run, a
//! rebuilt component reloads, and a stuck plugin stops without holding up
//! the window; and what goes on around those: the settings' preview, loads
//! cancelled, prompts gone stale or dismissed, spawn grants, commands
//! checked and the deadline's ticker. The plugins are kiln-plugin's WAT
//! fixtures and one of the tests' own.

// What only its Unix tests use goes unused elsewhere.
#![cfg_attr(not(unix), allow(unused_imports))]

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_command::{
    ContextFrame, ContextStack, Invocation, Value,
    capability::{Capability, Grants},
};
use fe_config::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Action as _, App, Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::{
    ApiVersion,
    extension::{ExtensionApi, load_manifest},
};
use kiln::platform::config::batch_channel;
use kiln_plugin::{
    bindings::Error,
    grants::{GrantStore, resolve_program},
    limits::CALL_DEADLINE,
    manifest::{ExtensionOrigin, PluginManifest},
    runner::{Call, Reply},
};

use super::{
    CACHE_DIR, Found, GRANTS_FILE, PluginAnswer, PluginHost, PluginState, StopReason, lock,
    pending_plugin_grants, plugin_kind, plugin_states, prompt_finished, reevaluate,
    set_plugin_dirs, set_plugins, set_preview, set_workspace_roots, start_ticker, unload_plugin,
};
use crate::{Launch, PluginAsk, Workbench};

/// Registers `acme.tool.hello` (with args), `acme.tool.spin`, which never
/// returns, and `acme.tool.invoke`.
const COMMANDS: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/commands.wat");

/// Registers nothing; its `run-command` never returns.
const LOOP_FOREVER: &str =
    include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/loop_forever.wat");

/// The commands fixture's plugin id.
const TOOL: &str = "acme.tool";

/// A test's plugins: its state, cache and extensions directories.
struct Plugins {
    dir: tempfile::TempDir,
}

impl Plugins {
    fn cache_entries(&self) -> usize {
        fs::read_dir(self.dir.path().join("cache").join(CACHE_DIR))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "cwasm"))
                    .count()
            })
            .unwrap_or(0)
    }

    fn store(&self) -> GrantStore {
        GrantStore::new(self.dir.path().join("state").join(GRANTS_FILE))
    }

    /// Plugin `id` at `version`, its component `wat`, asking for
    /// `capabilities`, written to its own extension directory.
    fn install(&self, id: &str, version: &str, wat: &str, capabilities: &[&str]) -> Found {
        let dir = self.dir.path().join("extensions").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.wasm"), wat).unwrap();
        let capabilities: Vec<String> = capabilities
            .iter()
            .map(|capability| format!("{capability:?}"))
            .collect();
        fs::write(
            dir.join("extension.toml"),
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\nlicense = \"MIT\"\n\n\
                 [engine]\ntenmoku = \"1.1\"\n\n\
                 [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [{}]\n",
                capabilities.join(", ")
            ),
        )
        .unwrap();
        found(&dir)
    }

    /// Grants plugin `id` at `version` `capabilities`, as Allow would.
    fn grant(&self, id: &str, version: (u32, u32, u32), capabilities: &[&str]) {
        self.store()
            .grant(id, version, &grants(capabilities), &[], false)
            .unwrap();
    }
}

fn grants(capabilities: &[&str]) -> fe_command::Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// The plugin in extension `dir`, as the host is handed it.
fn found(dir: &Path) -> Found {
    let api = ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = load_manifest(dir, &api);
    let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    Found::new(&manifest, Ok(plugin))
}

/// The app with the workbench's commands, and the plugin host keeping its
/// files in a directory of the test's, with the preview on.
fn start(cx: &mut TestAppContext) -> Plugins {
    // Loading runs under the call deadline too: a starved thread can push
    // instantiate and init past the real 100 ms and stop the plugin for good.
    start_with_call_deadline(cx, Duration::from_secs(30))
}

/// `start`, with the plugin host's call deadline set to `deadline`: tests
/// whose subject is the deadline pass `CALL_DEADLINE`.
fn start_with_call_deadline(cx: &mut TestAppContext, deadline: Duration) -> Plugins {
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let state: PathBuf = dir.path().join("state");
    let cache = dir.path().join("cache");
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_plugin_dirs(cx, state, cache);
        set_preview(cx, true);
        cx.global_mut::<PluginHost>().limits.call_deadline = deadline;
    });
    Plugins { dir }
}

/// A workbench with no folder, focused, in a window of its own, active.
fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, vcx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(vcx);
    (workbench, vcx)
}

fn draw(vcx: &mut VisualTestContext) {
    for _ in 0..2 {
        vcx.run_until_parked();
        vcx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// Runs until `done`: plugins load and stop on their own threads.
fn until(cx: &mut TestAppContext, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if cx.update(|cx| done(cx)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:#?}",
            cx.update(|cx| plugin_states(cx))
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn state(id: &str, cx: &App) -> Option<PluginState> {
    plugin_states(cx)
        .into_iter()
        .find(|info| info.id == id)
        .map(|info| info.state)
}

fn running(id: &'static str) -> impl Fn(&App) -> bool {
    move |cx| state(id, cx) == Some(PluginState::Running)
}

/// The plugins' inits and shutdowns, in order.
fn lifecycle(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        lock(&cx.global::<PluginHost>().record)
            .iter()
            .filter(|line| line.starts_with("init ") || line.starts_with("shutdown "))
            .cloned()
            .collect()
    })
}

/// What the plugins logged.
fn logged(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        lock(&cx.global::<PluginHost>().record)
            .iter()
            .filter(|line| line.starts_with("log "))
            .cloned()
            .collect()
    })
}

/// The `acme.*` commands the palette lists in the window's focus.
fn listed(vcx: &mut VisualTestContext) -> Vec<String> {
    draw(vcx);
    vcx.update(|window, cx| {
        let stack = kiln_platform::keys::context_stack(&window.context_stack(), cx);
        let mut ids: Vec<String> = kiln_shell::command_palette::commands_in(&stack, cx)
            .into_iter()
            .map(|entry| entry.id.to_string())
            .filter(|id| id.starts_with("acme."))
            .collect();
        ids.sort();
        ids
    })
}

/// Dispatches `command` with `args` in the window, as from a focused
/// editor.
fn run(vcx: &mut VisualTestContext, command: &str, args: Value) {
    let editor = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    run_in(vcx, command, args, editor);
}

fn run_in(vcx: &mut VisualTestContext, command: &str, args: Value, context: ContextStack) {
    let invocation = Invocation::new(command.parse().unwrap())
        .args(args)
        .context(context);
    vcx.update(|window, cx| window.dispatch_action(RunCommand(invocation).boxed_clone(), cx));
}

#[gpui::test]
fn a_plugin_is_pending_until_granted(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    cx.update(|cx| set_plugins(cx, vec![tool.clone()]));
    cx.run_until_parked();

    // New: it waits, compiled and loaded nothing.
    assert_eq!(
        cx.update(|cx| state(TOOL, cx)),
        Some(PluginState::WaitingForGrant)
    );
    let pending = cx.update(|cx| pending_plugin_grants(cx));
    assert_eq!(pending.len(), 1, "{pending:#?}");
    assert_eq!(pending[0].ask, PluginAsk::Install);
    assert_eq!(pending[0].requested, grants(&["commands"]));
    assert_eq!(plugins.cache_entries(), 0);
    assert_eq!(lifecycle(cx), Vec::<String>::new());
    // Handed it again, it still waits.
    cx.update(|cx| set_plugins(cx, vec![tool]));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| state(TOOL, cx)),
        Some(PluginState::WaitingForGrant)
    );
    assert_eq!(plugins.cache_entries(), 0);

    // Granted, it loads.
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    cx.update(reevaluate);
    until(cx, "the granted plugin to run", running(TOOL));
    assert_eq!(lifecycle(cx), [format!("init {TOOL}")]);
    // Windows doesn't cache compiled plugins (see CONTRIBUTING.md).
    assert_eq!(plugins.cache_entries(), usize::from(cfg!(unix)));
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)), []);
    let info = cx.update(|cx| plugin_states(cx)).remove(0);
    assert_eq!(info.granted, grants(&["commands"]));
}

#[gpui::test]
fn the_prompt_follows_the_pending_grant_and_records_the_answer(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let other = plugins.install("acme.other", "2.0.0", LOOP_FOREVER, &["status"]);
    let third = plugins.install("acme.third", "3.0.0", LOOP_FOREVER, &["status"]);
    {
        let (_, vcx) = window(cx);
        vcx.update(|_, cx| set_plugins(cx, vec![tool.clone()]));
        draw(vcx);
        let (question, detail) = vcx.pending_prompt().expect("the plugin's prompt");
        assert_eq!(question, "Allow the plugin acme.tool to load?");
        assert!(detail.contains("commands"), "{detail}");
        // Handed the plugins again while it asks: it asks once.
        vcx.update(|_, cx| set_plugins(cx, vec![tool.clone()]));
        draw(vcx);
        vcx.simulate_prompt_answer("Allow");
        draw(vcx);
        assert!(!vcx.has_pending_prompt(), "asked once");
        until(vcx, "the allowed plugin to run", running(TOOL));
        let record = plugins.store().get(TOOL).unwrap().unwrap();
        assert_eq!(record.version, Some((1, 0, 0)));
        assert_eq!(record.granted, grants(&["commands"]));

        // Don't Allow records the decline; the same request isn't asked
        // again.
        vcx.update(|_, cx| set_plugins(cx, vec![tool.clone(), other.clone()]));
        draw(vcx);
        let (question, _) = vcx.pending_prompt().expect("the second plugin's prompt");
        assert_eq!(question, "Allow the plugin acme.other to load?");
        vcx.simulate_prompt_answer("Don't Allow");
        until(vcx, "the decline", |cx| {
            state("acme.other", cx) == Some(PluginState::Declined)
        });
        let record = plugins.store().get("acme.other").unwrap().unwrap();
        assert_eq!(record.declined, Some(grants(&["status"])));
        assert_eq!(record.version, None);
        vcx.update(|_, cx| reevaluate(cx));
        draw(vcx);
        assert!(!vcx.has_pending_prompt());
        assert_eq!(vcx.update(|_, cx| pending_plugin_grants(cx)), []);

        // A prompt closed without an answer leaves it pending.
        vcx.update(|_, cx| set_plugins(cx, vec![tool.clone(), other.clone(), third.clone()]));
        draw(vcx);
        let (question, _) = vcx.pending_prompt().expect("the third plugin's prompt");
        assert_eq!(question, "Allow the plugin acme.third to load?");
        vcx.update(|window, _| window.remove_window());
        vcx.run_until_parked();
        vcx.simulate_prompt_answer("Allow");
        vcx.run_until_parked();
    }
    assert_eq!(
        cx.update(|cx| state("acme.third", cx)),
        Some(PluginState::WaitingForGrant)
    );
    assert_eq!(plugins.store().get("acme.third").unwrap(), None);
    // A window activated later asks about it.
    let (_, vcx) = window(cx);
    let (question, _) = vcx.pending_prompt().expect("asked again in the new window");
    assert_eq!(question, "Allow the plugin acme.third to load?");
}

#[gpui::test]
fn an_escalating_update_keeps_the_old_version_loaded(cx: &mut TestAppContext) {
    let plugins = start(cx);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    until(vcx, "the granted plugin to run", running(TOOL));
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);

    // 1.1.0 asks for more: 1.0.0 runs on while the user is asked.
    let update = format!("{COMMANDS}\n;; 1.1.0\n");
    let update = plugins.install(TOOL, "1.1.0", &update, &["commands", "status"]);
    vcx.update(|_, cx| set_plugins(cx, vec![update]));
    draw(vcx);
    let (question, detail) = vcx.pending_prompt().expect("the update's prompt");
    assert_eq!(question, "Allow acme.tool 1.1.0 more access?");
    assert!(detail.contains("status"), "{detail}");
    let pending = vcx.update(|_, cx| pending_plugin_grants(cx));
    assert_eq!(
        pending[0].ask,
        PluginAsk::Escalation {
            added: grants(&["status"]),
            keep_running: Some((1, 0, 0)),
        }
    );
    assert_eq!(
        vcx.update(|_, cx| state(TOOL, cx)),
        Some(PluginState::Running)
    );
    assert_eq!(lifecycle(vcx), [format!("init {TOOL}")]);
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);

    // Allowed, the new version replaces it.
    vcx.simulate_prompt_answer("Allow");
    until(vcx, "the update to load", |_| {
        plugins
            .store()
            .get(TOOL)
            .unwrap()
            .is_some_and(|record| record.version == Some((1, 1, 0)))
    });
    let lifecycle_now = |vcx: &mut VisualTestContext| lifecycle(vcx).len();
    let deadline = Instant::now() + Duration::from_secs(30);
    while lifecycle_now(vcx) < 3 {
        assert!(Instant::now() < deadline, "the update didn't load");
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        lifecycle(vcx),
        [
            format!("init {TOOL}"),
            format!("shutdown {TOOL}"),
            format!("init {TOOL}")
        ]
    );
    until(vcx, "the update to run", running(TOOL));
    let info = vcx.update(|_, cx| plugin_states(cx)).remove(0);
    assert_eq!(info.version, (1, 1, 0));
    assert_eq!(info.granted, grants(&["commands", "status"]));
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);
}

#[gpui::test]
fn a_granted_plugin_registers_its_commands_and_runs_them(cx: &mut TestAppContext) {
    let plugins = start(cx);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (workbench, vcx) = window(cx);
    assert_eq!(listed(vcx), Vec::<String>::new());
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    until(vcx, "the granted plugin to run", running(TOOL));

    // Listed as `<id>.<name>`; `hello` takes args, which the palette
    // leaves out.
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);

    // A workbench runs one on the plugin's thread.
    run(vcx, "acme.tool.hello", serde_json::json!({"a": 1}));
    let deadline = Instant::now() + Duration::from_secs(30);
    while logged(vcx).len() < 2 {
        assert!(Instant::now() < deadline, "the command didn't run");
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        logged(vcx),
        [
            format!("log {TOOL} on plugin {TOOL}: acme.tool.hello"),
            format!("log {TOOL} on plugin {TOOL}: {{\"a\":1}}"),
        ]
    );
    let propagated = workbench.read_with(vcx, |workbench, _| {
        (workbench.commands_run.iter()).any(|run| run.command.as_str() == "acme.tool.hello")
    });
    assert!(!propagated, "the plugin host took it");

    // Unloaded, its commands leave the list.
    vcx.update(|_, cx| unload_plugin(TOOL, cx));
    until(vcx, "the plugin to unload", |cx| {
        matches!(state(TOOL, cx), Some(PluginState::Stopped(diagnostic))
            if diagnostic.reason == StopReason::Unloaded)
    });
    assert_eq!(listed(vcx), Vec::<String>::new());
    assert_eq!(
        lifecycle(vcx),
        [format!("init {TOOL}"), format!("shutdown {TOOL}")]
    );
    // Handed again unchanged, it stays unloaded; reevaluated, it's back.
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    draw(vcx);
    assert!(matches!(
        vcx.update(|_, cx| state(TOOL, cx)),
        Some(PluginState::Stopped(_))
    ));
    vcx.update(|_, cx| reevaluate(cx));
    until(vcx, "the plugin to run again", running(TOOL));
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);
}

#[gpui::test]
fn a_rebuilt_component_reloads_without_a_prompt(cx: &mut TestAppContext) {
    let plugins = start(cx);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    until(vcx, "the granted plugin to run", running(TOOL));
    assert_eq!(lifecycle(vcx), [format!("init {TOOL}")]);

    // Rebuilt with the same capabilities, as the extensions service hands
    // it again when its component changes.
    let rebuilt = format!("{COMMANDS}\n;; rebuilt\n");
    let rebuilt = plugins.install(TOOL, "1.0.0", &rebuilt, &["commands"]);
    vcx.update(|_, cx| set_plugins(cx, vec![rebuilt]));
    let deadline = Instant::now() + Duration::from_secs(30);
    while lifecycle(vcx).len() < 3 {
        assert!(Instant::now() < deadline, "the rebuilt plugin didn't load");
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        lifecycle(vcx),
        [
            format!("init {TOOL}"),
            format!("shutdown {TOOL}"),
            format!("init {TOOL}")
        ]
    );
    until(vcx, "the rebuilt plugin to run", running(TOOL));
    draw(vcx);
    assert!(!vcx.has_pending_prompt());
    assert_eq!(vcx.update(|_, cx| pending_plugin_grants(cx)), []);
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);
}

#[gpui::test]
fn a_stuck_plugin_does_not_freeze_the_window(cx: &mut TestAppContext) {
    let plugins = start_with_call_deadline(cx, CALL_DEADLINE);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (workbench, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    until(vcx, "the granted plugin to run", running(TOOL));

    // `spin` never returns: running it doesn't wait for it, and the window
    // takes the next command meanwhile.
    let started = Instant::now();
    run(vcx, "acme.tool.spin", Value::Null);
    run(vcx, "test.next", Value::Null);
    assert!(started.elapsed() < CALL_DEADLINE, "{:?}", started.elapsed());
    let next = workbench.read_with(vcx, |workbench, _| {
        (workbench.commands_run.iter()).any(|run| run.command.as_str() == "test.next")
    });
    assert!(next, "the window took the next command");

    // Its deadline stops it.
    until(vcx, "the stuck plugin to stop", |cx| {
        matches!(state(TOOL, cx), Some(PluginState::Stopped(diagnostic))
            if diagnostic.reason == StopReason::Timeout)
    });
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(listed(vcx), Vec::<String>::new());
    // Handed again unchanged, it stays stopped.
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    draw(vcx);
    assert!(matches!(
        vcx.update(|_, cx| state(TOOL, cx)),
        Some(PluginState::Stopped(_))
    ));
}

/// Each load waits for a message on the returned sender before it starts.
fn gate_loads(cx: &mut TestAppContext) -> async_channel::Sender<()> {
    let (sender, receiver) = async_channel::unbounded();
    cx.update(|cx| cx.global_mut::<PluginHost>().load_gate = Some(receiver));
    sender
}

/// Whether a load or an unload of plugin `id` is under way.
fn busy(id: &str, cx: &App) -> bool {
    cx.global::<PluginHost>()
        .slots
        .get(id)
        .is_some_and(|slot| slot.busy)
}

/// The roots plugin `id`'s running load reaches, if it reaches files.
fn loaded_roots(id: &str, cx: &App) -> Option<Vec<PathBuf>> {
    let slot = cx.global::<PluginHost>().slots.get(id)?;
    slot.loaded.as_ref()?.identity.workspace.clone()
}

/// Runs until plugin `id`'s lifecycle has `count` entries.
fn until_lifecycle(cx: &mut TestAppContext, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while lifecycle(cx).len() < count {
        assert!(
            Instant::now() < deadline,
            "lifecycle stayed {:?}",
            lifecycle(cx)
        );
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn the_preview_setting_shuts_plugins_down_live(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    fs::write(&settings, "").unwrap();
    let (sender, batches) = batch_channel();
    let config = kiln::Config::new("dev.fastedit.test", "fastedit")
        .commands(|cx| {
            fe_gpui::init(cx);
            crate::init(cx);
        })
        .settings_store(SettingsStore::load(Some(settings.clone()), None).into_inner())
        .theme_mode(kiln::ui::ThemeMode::Dark)
        .watch_with(batches);
    let (state_dir, cache_dir) = (dir.path().join("state"), dir.path().join("cache"));
    cx.update(|cx| {
        kiln::init(cx, config);
        set_plugin_dirs(cx, state_dir, cache_dir);
    });
    let plugins = Plugins { dir };
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    draw(vcx);
    // Off by default: granted, it's neither compiled nor loaded.
    assert_eq!(
        vcx.update(|_, cx| state(TOOL, cx)),
        Some(PluginState::PreviewOff)
    );
    assert_eq!(plugins.cache_entries(), 0);
    let save = |text: &str, vcx: &mut VisualTestContext| {
        fs::write(&settings, text).unwrap();
        sender.send(vec![settings.clone()]);
        draw(vcx);
    };

    // On: it loads and its commands are listed.
    save("[extensions]\nplugins_preview = true\n", vcx);
    until(vcx, "the plugin to load", running(TOOL));
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);

    // Off: it reads as off only once its shutdown has run, its commands
    // withdrawn and its grant kept.
    save("", vcx);
    until(vcx, "the plugin to unload", |cx| {
        state(TOOL, cx) == Some(PluginState::PreviewOff)
    });
    assert_eq!(
        lifecycle(vcx),
        [format!("init {TOOL}"), format!("shutdown {TOOL}")]
    );
    assert_eq!(listed(vcx), Vec::<String>::new());
    let record = plugins.store().get(TOOL).unwrap().unwrap();
    assert_eq!(record.granted, grants(&["commands"]));

    // On again: it loads without asking.
    save("[extensions]\nplugins_preview = true\n", vcx);
    until(vcx, "the plugin to load again", running(TOOL));
    assert!(!vcx.has_pending_prompt());
    assert_eq!(listed(vcx), ["acme.tool.invoke", "acme.tool.spin"]);
}

#[gpui::test]
fn a_load_cancelled_before_init_runs_none_of_the_plugin(cx: &mut TestAppContext) {
    let plugins = start(cx);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let gate = gate_loads(cx);
    cx.update(|cx| set_plugins(cx, vec![tool]));
    cx.run_until_parked();
    assert!(cx.update(|cx| busy(TOOL, cx)), "the load waits at the gate");

    // The preview goes off while it waits: nothing of it runs.
    cx.update(|cx| set_preview(cx, false));
    gate.try_send(()).unwrap();
    until(cx, "the cancelled load to end", |cx| {
        !busy(TOOL, cx) && state(TOOL, cx) == Some(PluginState::PreviewOff)
    });
    assert_eq!(lifecycle(cx), Vec::<String>::new());
    assert_eq!(plugins.cache_entries(), 0);
}

#[gpui::test]
fn a_load_for_roots_since_changed_is_cancelled_and_loaded_again(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let (old_root, new_root) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let capabilities = ["commands", "fs.read:workspace"];
    plugins.grant(TOOL, (1, 0, 0), &capabilities);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &capabilities);
    cx.update(|cx| set_workspace_roots(cx, vec![old_root.path().to_owned()]));
    let gate = gate_loads(cx);
    cx.update(|cx| set_plugins(cx, vec![tool]));
    cx.run_until_parked();
    assert!(cx.update(|cx| busy(TOOL, cx)), "the load waits at the gate");

    // The roots change while it waits: it's cancelled, and a load on the
    // new roots follows.
    cx.update(|cx| set_workspace_roots(cx, vec![new_root.path().to_owned()]));
    cx.run_until_parked();
    gate.try_send(()).unwrap();
    gate.try_send(()).unwrap();
    until(cx, "the load on the new roots", running(TOOL));
    assert_eq!(lifecycle(cx), [format!("init {TOOL}")]);
    assert_eq!(
        cx.update(|cx| loaded_roots(TOOL, cx)),
        Some(vec![new_root.path().to_owned()])
    );
}

#[gpui::test]
fn unloading_during_a_load_keeps_it_stopped(cx: &mut TestAppContext) {
    let plugins = start(cx);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let gate = gate_loads(cx);
    cx.update(|cx| set_plugins(cx, vec![tool.clone()]));
    cx.run_until_parked();
    cx.update(|cx| unload_plugin(TOOL, cx));
    // Handed again unchanged, and decided, before the cancelled load ends.
    cx.update(|cx| set_plugins(cx, vec![tool.clone()]));
    cx.run_until_parked();
    gate.try_send(()).unwrap();
    let unloaded = |cx: &App| {
        matches!(state(TOOL, cx), Some(PluginState::Stopped(diagnostic))
            if diagnostic.reason == StopReason::Unloaded)
    };
    until(cx, "the unload", |cx| !busy(TOOL, cx) && unloaded(cx));

    // Handed again unchanged, it stays stopped: no load starts.
    cx.update(|cx| set_plugins(cx, vec![tool]));
    cx.run_until_parked();
    assert!(!cx.update(|cx| busy(TOOL, cx)));
    assert!(cx.update(|cx| unloaded(cx)));
    assert_eq!(lifecycle(cx), Vec::<String>::new());
}

#[gpui::test]
fn a_kept_version_follows_the_roots_while_its_update_waits(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let capabilities = ["commands", "fs.read:workspace"];
    plugins.grant(TOOL, (1, 0, 0), &capabilities);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &capabilities);
    cx.update(|cx| {
        set_workspace_roots(cx, vec![first.path().to_owned()]);
        set_plugins(cx, vec![tool]);
    });
    until(cx, "the plugin to run", running(TOOL));

    // 1.1.0 asks for more: 1.0.0 runs on, and follows the roots.
    let more = ["commands", "fs.read:workspace", "status"];
    let update = format!("{COMMANDS}\n;; 1.1.0\n");
    let update = plugins.install(TOOL, "1.1.0", &update, &more);
    cx.update(|cx| set_plugins(cx, vec![update.clone()]));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)).len(), 1);
    cx.update(|cx| set_workspace_roots(cx, vec![second.path().to_owned()]));
    until_lifecycle(cx, 3);
    until(cx, "the kept version on the new roots", |cx| {
        loaded_roots(TOOL, cx) == Some(vec![second.path().to_owned()])
    });
    let info = cx.update(|cx| plugin_states(cx)).remove(0);
    assert_eq!(info.state, PluginState::Running);
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)).len(), 1);
    let kept = cx.update(|cx| {
        let slot = &cx.global::<PluginHost>().slots[TOOL];
        slot.loaded.as_ref().unwrap().identity.version
    });
    assert_eq!(kept, (1, 0, 0));

    // Declined, it still runs, and still follows them.
    plugins.store().decline(TOOL, &grants(&more)).unwrap();
    cx.update(|cx| set_plugins(cx, vec![update]));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)), []);
    cx.update(|cx| set_workspace_roots(cx, Vec::new()));
    until_lifecycle(cx, 5);
    until(cx, "the kept version on no roots", |cx| {
        loaded_roots(TOOL, cx) == Some(Vec::new())
    });
    assert_eq!(cx.update(|cx| state(TOOL, cx)), Some(PluginState::Running));
}

#[gpui::test]
fn a_stale_prompt_records_nothing(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let net = plugins.install("acme.net", "1.0.0", LOOP_FOREVER, &["net:*"]);
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![net]));
    draw(vcx);
    let (question, _) = vcx.pending_prompt().expect("the plugin's prompt");
    assert_eq!(question, "Allow the plugin acme.net to load?");

    // The preview goes off with the prompt up: Allow neither records nor
    // goes on to the full-access prompt.
    vcx.update(|_, cx| set_preview(cx, false));
    draw(vcx);
    vcx.simulate_prompt_answer("Allow");
    draw(vcx);
    assert!(!vcx.has_pending_prompt(), "no full-access prompt");
    assert_eq!(plugins.store().get("acme.net").unwrap(), None);
    assert_eq!(
        vcx.update(|_, cx| state("acme.net", cx)),
        Some(PluginState::PreviewOff)
    );
}

#[gpui::test]
fn a_dismissed_prompt_is_not_reopened_in_its_window(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    {
        let (_, vcx) = window(cx);
        vcx.update(|_, cx| set_plugins(cx, vec![tool.clone()]));
        draw(vcx);
        assert!(vcx.has_pending_prompt());
        let pending = vcx.update(|_, cx| pending_plugin_grants(cx)).remove(0);

        // Closed without an answer, with the window open: it stays pending,
        // and this window doesn't ask again.
        vcx.update(|window, cx| {
            prompt_finished(window.window_handle(), pending, PluginAnswer::Pending, cx);
        });
        draw(vcx);
        // The platform's prompt, answered now, decides nothing.
        vcx.simulate_prompt_answer("Allow");
        draw(vcx);
        assert!(!vcx.has_pending_prompt(), "not reopened");
        vcx.update(|_, cx| set_plugins(cx, vec![tool.clone()]));
        draw(vcx);
        assert!(!vcx.has_pending_prompt(), "not reopened on a change");
        assert_eq!(plugins.store().get(TOOL).unwrap(), None);
        assert_eq!(
            vcx.update(|_, cx| state(TOOL, cx)),
            Some(PluginState::WaitingForGrant)
        );
    }
    // A window activated later asks.
    let (_, vcx) = window(cx);
    let (question, _) = vcx.pending_prompt().expect("asked in the new window");
    assert_eq!(question, "Allow the plugin acme.tool to load?");
}

/// A plugin whose `run-command` spawns the program its args name, with no
/// args or stdin, and logs what it wrote to stdout; the command fails with
/// the spawn's error.
#[cfg(unix)]
const SPAWN: &str = r#"
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

  (import "kiln:extension/log@1.0.0" (instance $log
    (type $level (enum "trace" "debug" "info" "warn" "error"))
    (export "level" (type $level' (eq $level)))
    (export "log" (func (param "level" $level') (param "message" string)))))

  (import "kiln:extension/process@1.0.0" (instance $process
    (alias outer $C $error (type $error))
    (type $output (record
      (field "status" (option s32))
      (field "stdout" (list u8))
      (field "stderr" (list u8))))
    (export "output" (type $output' (eq $output)))
    (export "spawn" (func
      (param "program" string)
      (param "args" (list string))
      (param "stdin" (option (list u8)))
      (result (result $output' (error $error)))))))

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
    ;; Bytes 0..8 stay zero: a `result` that is `ok`. 64..92 hold the spawn's
    ;; result; the heap starts at 1024.
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

  (alias export $log "log" (func $log-log))
  (alias export $process "spawn" (func $process-spawn))
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core func $spawn (canon lower (func $process-spawn) (memory $memory) (realloc $realloc)))
  (core instance $host
    (export "log" (func $log))
    (export "spawn" (func $spawn)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (import "host" "spawn" (func $spawn (param i32 i32 i32 i32 i32 i32 i32 i32)))
    (func (export "init") (result i32) (i32.const 0))
    (func (export "shutdown"))
    ;; The spawn's result at 64: its case at 64, then an `output` at 68
    ;; (its stdout's pointer at 76 and length at 80), or an `error` laid out
    ;; as the command's own `result` returns one.
    (func (export "run-command") (param $id i32) (param $id-len i32) (param $args i32) (param $args-len i32) (result i32)
      (call $spawn
        (local.get $args) (local.get $args-len)
        (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 64))
      (if (i32.eqz (i32.load8_u (i32.const 64)))
        (then (call $log (i32.const 2) (i32.load (i32.const 76)) (i32.load (i32.const 80)))))
      (i32.const 64))
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

#[gpui::test]
#[cfg(unix)]
fn a_granted_program_runs_and_an_ungranted_one_is_denied(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let plugins = start(cx);
    let bin = tempfile::tempdir().unwrap();
    for (name, said) in [("greet", "hi"), ("other", "no")] {
        let path = bin.path().join(name);
        fs::write(&path, format!("#!/bin/sh\necho {said}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let greet = resolve_program("greet", bin.path().as_os_str()).unwrap();
    let requested: Grants = grants(&["process.spawn:greet"]);
    plugins
        .store()
        .grant(SPAWNER, (1, 0, 0), &requested, &[greet], false)
        .unwrap();
    let spawner = plugins.install(SPAWNER, "1.0.0", SPAWN, &["process.spawn:greet"]);
    cx.update(|cx| set_plugins(cx, vec![spawner]));
    until(cx, "the plugin to run", running(SPAWNER));
    let plugin = cx.update(|cx| {
        let slot = &cx.global::<PluginHost>().slots[SPAWNER];
        slot.loaded.as_ref().unwrap().plugin.clone()
    });
    let call = |program: &str| {
        lock(&plugin).call(Call::RunCommand {
            id: format!("{SPAWNER}.run"),
            args: program.to_owned(),
        })
    };

    // Granted: it runs, as recorded.
    assert_eq!(call("greet"), Ok(Reply::RunCommand(Ok(()))));
    assert_eq!(
        logged(cx),
        [format!("log {SPAWNER} on plugin {SPAWNER}: hi\n")]
    );
    // Not granted: denied, and nothing runs.
    let denied = call("other");
    assert!(
        matches!(&denied, Ok(Reply::RunCommand(Err(Error::Denied(_))))),
        "{denied:?}"
    );
    assert_eq!(logged(cx).len(), 1);
}

#[gpui::test]
fn a_command_runs_only_in_its_context_with_args_that_fit(cx: &mut TestAppContext) {
    let plugins = start_with_call_deadline(cx, CALL_DEADLINE);
    plugins.grant(TOOL, (1, 0, 0), &["commands"]);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    until(vcx, "the plugin to run", running(TOOL));

    // `hello` applies in an editor that isn't read-only, and takes a table.
    let args = serde_json::json!({"a": 1});
    run_in(vcx, "acme.tool.hello", args.clone(), ContextStack::new());
    let read_only = ContextStack::new().with(ContextFrame::new("editor").flag("read_only"));
    run_in(vcx, "acme.tool.hello", args, read_only);
    run(vcx, "acme.tool.hello", serde_json::json!([1]));
    run(vcx, "acme.tool.hello", serde_json::json!({"a": null}));
    // `spin` takes no args: refused, it never runs, so never times out.
    run(vcx, "acme.tool.spin", serde_json::json!({"turns": 1}));
    draw(vcx);
    std::thread::sleep(CALL_DEADLINE * 2);
    draw(vcx);
    assert_eq!(logged(vcx), Vec::<String>::new());
    assert_eq!(
        vcx.update(|_, cx| state(TOOL, cx)),
        Some(PluginState::Running)
    );

    // In its context with args that fit, it runs.
    run(vcx, "acme.tool.hello", serde_json::json!({"a": 1}));
    let deadline = Instant::now() + Duration::from_secs(30);
    while logged(vcx).len() < 2 {
        assert!(Instant::now() < deadline, "the command didn't run");
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn no_deadline_ticker_no_plugin(cx: &mut TestAppContext) {
    // A ticker thread that can't start is an error, not a silent no-op.
    let engine = kiln_plugin::engine::PluginEngine::shared().unwrap();
    let started = start_ticker(engine.engine().clone(), |_| {
        Err(std::io::Error::other("no threads left"))
    });
    let error = started.unwrap_err();
    assert!(error.contains("no thread to enforce"), "{error}");

    // And a load without one refuses to run the plugin.
    let plugins = start(cx);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (bridge, _) = crate::plugin_bridge::Bridge::new();
    let commands = kiln_plugin::commands::CommandHost::new(Vec::new(), bridge.dispatcher());
    let context = cx.update(|cx| super::LoadContext {
        cache: None,
        commands,
        bridge,
        limits: kiln_plugin::limits::Limits::default(),
        secrets: std::sync::Arc::new(kiln::core::secrets::MemoryStore::new()),
        secrets_index: None,
        store: None,
        workspace: kiln_plugin::fs::Workspace::default(),
        record: super::record(cx.global::<PluginHost>()),
        paint: crate::plugin_paint::paint(cx),
        ticker: |_| Err("no thread to enforce plugins' deadlines: refused".into()),
    });
    let artifact = super::Artifact {
        manifest: tool.plugin.clone().unwrap(),
        bytes: COMMANDS.as_bytes().to_vec(),
        record: Default::default(),
    };
    let cancel = std::sync::Arc::default();
    let loaded = context.load(&artifact, &grants(&["commands"]), &cancel);
    match loaded {
        Err(super::LoadFailure::Failed(message)) => {
            assert!(message.contains("no thread to enforce"), "{message}");
        }
        Err(super::LoadFailure::Cancelled) => panic!("not cancelled"),
        Ok(_) => panic!("loaded without a ticker"),
    }
    assert_eq!(lifecycle(cx), Vec::<String>::new());
}

#[gpui::test]
fn a_kept_version_reloading_follows_every_root_change(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let roots: Vec<tempfile::TempDir> = (0..5).map(|_| tempfile::tempdir().unwrap()).collect();
    let root = |ix: usize| vec![roots[ix].path().to_owned()];
    let capabilities = ["commands", "fs.read:workspace"];
    plugins.grant(TOOL, (1, 0, 0), &capabilities);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &capabilities);
    cx.update(|cx| {
        set_workspace_roots(cx, root(0));
        set_plugins(cx, vec![tool]);
    });
    until(cx, "the plugin to run", running(TOOL));
    let more = ["commands", "fs.read:workspace", "status"];
    let update = format!("{COMMANDS}\n;; 1.1.0\n");
    let update = plugins.install(TOOL, "1.1.0", &update, &more);
    cx.update(|cx| set_plugins(cx, vec![update.clone()]));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)).len(), 1);
    let gate = gate_loads(cx);

    // Pending: the roots change, and change again while 1.0.0 reloads on
    // the first change, with the plugins decided again in between. It ends
    // up on the last roots, the load on the ones between never run.
    cx.update(|cx| set_workspace_roots(cx, root(1)));
    cx.run_until_parked();
    assert!(cx.update(|cx| busy(TOOL, cx)));
    cx.update(|cx| set_plugins(cx, vec![update.clone()]));
    cx.run_until_parked();
    cx.update(|cx| set_workspace_roots(cx, root(2)));
    cx.run_until_parked();
    gate.try_send(()).unwrap();
    gate.try_send(()).unwrap();
    until(cx, "1.0.0 on the last roots", |cx| {
        loaded_roots(TOOL, cx) == Some(root(2))
    });
    assert_eq!(
        lifecycle(cx),
        [
            format!("init {TOOL}"),
            format!("shutdown {TOOL}"),
            format!("init {TOOL}")
        ]
    );
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)).len(), 1);
    assert_eq!(cx.update(|cx| state(TOOL, cx)), Some(PluginState::Running));

    // Declined: the same.
    plugins.store().decline(TOOL, &grants(&more)).unwrap();
    cx.update(|cx| set_plugins(cx, vec![update.clone()]));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| pending_plugin_grants(cx)), []);
    cx.update(|cx| set_workspace_roots(cx, root(3)));
    cx.run_until_parked();
    cx.update(|cx| set_plugins(cx, vec![update]));
    cx.run_until_parked();
    cx.update(|cx| set_workspace_roots(cx, root(4)));
    cx.run_until_parked();
    gate.try_send(()).unwrap();
    gate.try_send(()).unwrap();
    until(cx, "1.0.0 on the last roots again", |cx| {
        loaded_roots(TOOL, cx) == Some(root(4))
    });
    assert_eq!(lifecycle(cx).len(), 5, "{:?}", lifecycle(cx));
    let kept = cx.update(|cx| {
        let slot = &cx.global::<PluginHost>().slots[TOOL];
        slot.loaded.as_ref().unwrap().identity.version
    });
    assert_eq!(kept, (1, 0, 0));
}

#[gpui::test]
fn a_prompt_for_a_replaced_request_records_nothing(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let tool = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands"]);
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    draw(vcx);
    assert!(vcx.has_pending_prompt());

    // The manifest asks for more now; the old prompt's Allow, before the
    // new request is decided, records nothing.
    let more = plugins.install(TOOL, "1.0.0", COMMANDS, &["commands", "status"]);
    vcx.update(|_, cx| set_plugins(cx, vec![more]));
    vcx.simulate_prompt_answer("Allow");
    draw(vcx);
    assert_eq!(plugins.store().get(TOOL).unwrap(), None);
    // The new request is asked about.
    let (_, detail) = vcx.pending_prompt().expect("the new request's prompt");
    assert!(detail.contains("status"), "{detail}");
}

/// An executable script `name` in `dir` running `body`, bound as a grant
/// would bind it.
#[cfg(unix)]
fn script(dir: &Path, name: &str, body: &str) -> kiln_plugin::grants::ProgramIdentity {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    resolve_program(name, dir.as_os_str()).unwrap()
}

/// The spawning plugin, granted `program`, running.
#[cfg(unix)]
fn spawner(
    cx: &mut TestAppContext,
    plugins: &Plugins,
    program: kiln_plugin::grants::ProgramIdentity,
) -> std::sync::Arc<std::sync::Mutex<super::HostedPlugin>> {
    let capability = format!("process.spawn:{}", program.name);
    plugins
        .store()
        .grant(
            SPAWNER,
            (1, 0, 0),
            &grants(&[&capability]),
            &[program],
            false,
        )
        .unwrap();
    let found = plugins.install(SPAWNER, "1.0.0", SPAWN, &[&capability]);
    cx.update(|cx| set_plugins(cx, vec![found]));
    until(cx, "the spawning plugin to run", running(SPAWNER));
    loaded_plugin(cx, SPAWNER)
}

#[cfg(unix)]
const SPAWNER: &str = "acme.spawn";

#[cfg(unix)]
fn loaded_plugin(
    cx: &mut TestAppContext,
    id: &str,
) -> std::sync::Arc<std::sync::Mutex<super::HostedPlugin>> {
    cx.update(|cx| {
        let slot = &cx.global::<PluginHost>().slots[id];
        slot.loaded.as_ref().unwrap().plugin.clone()
    })
}

#[cfg(unix)]
fn spawn(
    plugin: &std::sync::Mutex<super::HostedPlugin>,
    program: &str,
) -> Result<Reply, kiln_plugin::runner::CallError> {
    lock(plugin).call(Call::RunCommand {
        id: format!("{SPAWNER}.run"),
        args: program.to_owned(),
    })
}

#[gpui::test]
#[cfg(unix)]
fn a_granted_program_that_outlasts_the_call_deadline_completes(cx: &mut TestAppContext) {
    let plugins = start_with_call_deadline(cx, CALL_DEADLINE);
    let bin = tempfile::tempdir().unwrap();
    let plugin = spawner(
        cx,
        &plugins,
        script(bin.path(), "slow", "sleep 0.3\necho hi"),
    );
    // The run is longer than the call's 100ms deadline, which doesn't count it.
    assert_eq!(spawn(&plugin, "slow"), Ok(Reply::RunCommand(Ok(()))));
    assert_eq!(
        logged(cx),
        [format!("log {SPAWNER} on plugin {SPAWNER}: hi\n")]
    );
}

#[gpui::test]
#[cfg(unix)]
fn a_spawn_in_flight_ends_before_the_plugin_unloads(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let bin = tempfile::tempdir().unwrap();
    let plugin = spawner(cx, &plugins, script(bin.path(), "nap", "exec sleep 1"));

    // A preview turned off mid-spawn unloads the plugin once the run ends.
    let started = Instant::now();
    let napping = std::thread::spawn(move || spawn(&plugin, "nap"));
    std::thread::sleep(Duration::from_millis(20));
    cx.update(|cx| set_preview(cx, false));
    until(cx, "the plugin to unload", |cx| {
        state(SPAWNER, cx) == Some(PluginState::PreviewOff)
    });
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    napping.join().unwrap().ok();
}

#[gpui::test]
#[cfg(unix)]
fn a_reauthorized_program_reloads_the_plugin(cx: &mut TestAppContext) {
    let plugins = start(cx);
    let bin = tempfile::tempdir().unwrap();
    let plugin = spawner(cx, &plugins, script(bin.path(), "greet", "echo hi"));
    assert_eq!(spawn(&plugin, "greet"), Ok(Reply::RunCommand(Ok(()))));

    // The program changes: its grant is suspended, and the plugin asks
    // again, still running.
    let changed = script(bin.path(), "greet", "echo hello there");
    let refused = spawn(&plugin, "greet");
    assert!(
        matches!(&refused, Ok(Reply::RunCommand(Err(Error::Denied(_))))),
        "{refused:?}"
    );
    let found = plugins.install(SPAWNER, "1.0.0", SPAWN, &["process.spawn:greet"]);
    cx.update(|cx| set_plugins(cx, vec![found.clone()]));
    cx.run_until_parked();
    let pending = cx.update(|cx| pending_plugin_grants(cx));
    assert!(
        matches!(&pending[..], [pending] if matches!(pending.ask, PluginAsk::Escalation { .. })),
        "{pending:#?}"
    );
    assert_eq!(lifecycle(cx), [format!("init {SPAWNER}")]);

    // Allowed again, bound to the changed program: it reloads, and runs it.
    let requested = grants(&["process.spawn:greet"]);
    plugins
        .store()
        .grant(SPAWNER, (1, 0, 0), &requested, &[changed], false)
        .unwrap();
    cx.update(|cx| set_plugins(cx, vec![found]));
    until_lifecycle(cx, 3);
    until(cx, "the plugin to run again", running(SPAWNER));
    let plugin = loaded_plugin(cx, SPAWNER);
    assert_eq!(spawn(&plugin, "greet"), Ok(Reply::RunCommand(Ok(()))));
    assert_eq!(
        logged(cx).last().unwrap(),
        &format!("log {SPAWNER} on plugin {SPAWNER}: hello there\n")
    );
}

/// The napping plugin's id.
#[cfg(unix)]
const NAPPER: &str = "acme.napper";

/// A plugin that subscribes to `buffer-opened` in its `init`. Its handler
/// runs the granted program `nap`, then reads the opened buffer, logging its
/// text, and edits it to put an `X` first, logging `edited`; a failed step
/// logs `failed` at `error` and stops.
#[cfg(unix)]
const NAPPER_WAT: &str = r#"
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

  (import "kiln:extension/log@1.0.0" (instance $log
    (type $level (enum "trace" "debug" "info" "warn" "error"))
    (export "level" (type $level' (eq $level)))
    (export "log" (func (param "level" $level') (param "message" string)))))

  (import "kiln:extension/process@1.0.0" (instance $process
    (alias outer $C $error (type $error))
    (type $output (record
      (field "status" (option s32))
      (field "stdout" (list u8))
      (field "stderr" (list u8))))
    (export "output" (type $output' (eq $output)))
    (export "spawn" (func
      (param "program" string)
      (param "args" (list string))
      (param "stdin" (option (list u8)))
      (result (result $output' (error $error)))))))

  (import "fastedit:editor/buffer@1.0.0" (instance $buffer
    (alias outer $C $range (type $range))
    (alias outer $C $error (type $error))
    (type $snapshot (record
      (field "buffer" u64)
      (field "version" u64)
      (field "text" string)
      (field "path" (option string))
      (field "language" (option string))))
    (export "snapshot" (type $snapshot' (eq $snapshot)))
    (type $text-edit (record (field "range" $range) (field "text" string)))
    (export "text-edit" (type $text-edit' (eq $text-edit)))
    (export "read" (func
      (param "buffer" u64)
      (param "range" (option $range))
      (result (result $snapshot' (error $error)))))
    (export "edit" (func
      (param "buffer" u64)
      (param "version" u64)
      (param "edits" (list $text-edit'))
      (result (result u64 (error $error)))))))

  (import "kiln:extension/events@1.0.0" (instance $events
    (alias outer $C $range (type $range))
    (alias outer $C $error (type $error))
    (type $event-kinds (flags
      "buffer-opened" "buffer-changed" "buffer-saved" "buffer-closed"
      "focus-changed" "selection-changed" "config-reloaded"))
    (export "event-kinds" (type $event-kinds' (eq $event-kinds)))
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
    (export "event" (type (eq $event)))
    (export "subscribe" (func (param "kinds" $event-kinds') (result (result (error $error)))))))
  (alias export $events "event" (type $event))

  (core module $libc
    (memory (export "memory") 1)
    ;; Results are laid out at 64 (`subscribe`, `spawn`), 128 (`read`) and
    ;; 256 (`edit`); the edits at 320; the heap starts at 1024.
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

  (alias export $log "log" (func $log-log))
  (alias export $process "spawn" (func $process-spawn))
  (alias export $buffer "read" (func $buffer-read))
  (alias export $buffer "edit" (func $buffer-edit))
  (alias export $events "subscribe" (func $events-subscribe))
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core func $spawn (canon lower (func $process-spawn) (memory $memory) (realloc $realloc)))
  (core func $read (canon lower (func $buffer-read) (memory $memory) (realloc $realloc)))
  (core func $edit (canon lower (func $buffer-edit) (memory $memory) (realloc $realloc)))
  (core func $subscribe (canon lower (func $events-subscribe) (memory $memory) (realloc $realloc)))
  (core instance $host
    (export "log" (func $log))
    (export "spawn" (func $spawn))
    (export "read" (func $read))
    (export "edit" (func $edit))
    (export "subscribe" (func $subscribe)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (import "host" "spawn" (func $spawn (param i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "read" (func $read (param i64 i32 i64 i64 i32)))
    (import "host" "edit" (func $edit (param i64 i64 i32 i32 i32)))
    (import "host" "subscribe" (func $subscribe (param i32 i32)))
    (data (i32.const 352) "X")
    (data (i32.const 512) "nap")
    (data (i32.const 520) "edited")
    (data (i32.const 528) "failed")
    (func (export "init") (result i32)
      ;; `buffer-opened` is the first kind.
      (call $subscribe (i32.const 1) (i32.const 64))
      (i32.const 64))
    (func (export "shutdown"))
    (func (export "run-command") (param i32 i32 i32 i32) (result i32) (i32.const 0))
    (func $failed (call $log (i32.const 4) (i32.const 528) (i32.const 6)))
    (func (export "handle-events") (param $events i32) (param $count i32)
      (local $buffer i64)
      ;; Only a `buffer-opened`, tag 0, with its buffer at 8.
      (if (i32.ne (i32.load8_u (local.get $events)) (i32.const 0)) (then (return)))
      (local.set $buffer (i64.load (i32.add (local.get $events) (i32.const 8))))
      (call $spawn
        (i32.const 512) (i32.const 3)
        (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 64))
      (if (i32.ne (i32.load8_u (i32.const 64)) (i32.const 0)) (then (call $failed) (return)))
      ;; The snapshot in the `ok` at 128: its version at 144, its text at
      ;; 152 and 156.
      (call $read (local.get $buffer) (i32.const 0) (i64.const 0) (i64.const 0) (i32.const 128))
      (if (i32.ne (i32.load8_u (i32.const 128)) (i32.const 0)) (then (call $failed) (return)))
      (call $log (i32.const 2) (i32.load (i32.const 152)) (i32.load (i32.const 156)))
      ;; One edit at 320: the range 0..0, then the text at 352.
      (i64.store (i32.const 320) (i64.const 0))
      (i64.store (i32.const 328) (i64.const 0))
      (i32.store (i32.const 336) (i32.const 352))
      (i32.store (i32.const 340) (i32.const 1))
      (call $edit (local.get $buffer) (i64.load (i32.const 144)) (i32.const 320) (i32.const 1) (i32.const 256))
      (if (i32.ne (i32.load8_u (i32.const 256)) (i32.const 0)) (then (call $failed) (return)))
      (call $log (i32.const 2) (i32.const 520) (i32.const 6))))
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

/// An event handler's call deadline doesn't count the time its program
/// ran, on the buffers either: after a process longer than the deadline it
/// reads its buffer and edits it.
// Its program is a `/bin/sh` script, as `script` writes one.
#[cfg(unix)]
#[gpui::test]
fn an_event_handler_edits_its_buffer_after_a_slow_process(cx: &mut TestAppContext) {
    let plugins = start_with_call_deadline(cx, CALL_DEADLINE);
    let bin = tempfile::tempdir().unwrap();
    let nap = script(bin.path(), "nap", "sleep 0.3");
    let capabilities = ["process.spawn:nap", "buffer.read", "buffer.write"];
    plugins
        .store()
        .grant(NAPPER, (1, 0, 0), &grants(&capabilities), &[nap], false)
        .unwrap();
    let found = plugins.install(NAPPER, "1.0.0", NAPPER_WAT, &capabilities);
    let (workbench, vcx) = window(cx);
    vcx.update(|_, cx| set_plugins(cx, vec![found]));
    until(vcx, "the plugin to run", running(NAPPER));

    // Opening a file is the event; its handler outlasts the call deadline.
    let file = plugins.dir.path().join("a.txt");
    fs::write(&file, "alpha\n").unwrap();
    vcx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(file, window, cx));
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !logged(vcx).iter().any(|line| line.ends_with(": edited")) {
        assert!(
            Instant::now() < deadline,
            "the handler didn't finish: {:#?}",
            logged(vcx)
        );
        draw(vcx);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        logged(vcx),
        [
            format!("log {NAPPER} on plugin {NAPPER}: alpha\n"),
            format!("log {NAPPER} on plugin {NAPPER}: edited"),
        ]
    );
    let editor = vcx
        .update(|_, cx| workbench.read(cx).open_editors(cx))
        .remove(0);
    let text = editor.read_with(vcx, |editor, _| editor.model().buffer().text().to_string());
    assert_eq!(text, "Xalpha\n");
}
