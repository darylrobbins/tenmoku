//! Code plugins in the user's extensions directory run only while the
//! settings' `extensions.plugins_preview` is on (extensions record §7):
//! off, the default, none is compiled, loaded or asked about; the setting
//! applies with no restart; and a trusted folder's own extensions can't
//! bring one. The plugin is fe-plugin's prebuilt editor guest.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;
#[allow(dead_code)]
#[path = "../src/workspace_extensions.rs"]
mod workspace_extensions;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_command::capability::{Capability, Grants};
use fe_config::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{GRANTS_FILE, GrantStore, Launch, PluginState, Workbench};
use gpui::{App, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{extension::MANIFEST_FILE, trust::canonical},
    platform::config::{BatchSender, batch_channel, diagnostics},
    shell::trust,
    ui::ThemeMode,
};

/// The plugin's id.
const PLUGIN: &str = "acme.editor";

/// The editor guest as plugin [`PLUGIN`] in `extensions/editor`, asking to
/// read buffers.
fn install(extensions: &Path) -> PathBuf {
    let dir = extensions.join("editor");
    fs::create_dir_all(&dir).unwrap();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let guest = Path::new(&manifest).join("../fe-plugin/tests/guests/editor/editor.wasm");
    fs::copy(guest, dir.join("editor.wasm")).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "id = \"{PLUGIN}\"\nname = \"Editor\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.plugin]\nwasm = \"editor.wasm\"\ncapabilities = [\"buffer.read\"]\n"
        ),
    )
    .unwrap();
    dir
}

fn requested() -> Grants {
    [Capability::parse("buffer.read").unwrap()]
        .into_iter()
        .collect()
}

fn store(state: &Path) -> GrantStore {
    GrantStore::new(state.join("state").join(GRANTS_FILE))
}

/// Grants the plugin what it asks for, as Allow at its prompt would.
fn grant(state: &Path) {
    store(state)
        .grant(PLUGIN, (1, 0, 0), &requested(), &[], false)
        .unwrap();
}

/// The compiled components cached under `state/cache`.
fn cached(state: &Path) -> usize {
    fs::read_dir(state.join("cache").join(fe_workbench::CACHE_DIR))
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

/// The app over `state`: fastedit's commands and config with the user's
/// extensions in `state/extensions`, the user's settings in
/// `state/settings.toml` and the plugins' files under `state`, following
/// the workspace root `root` if any.
fn load(cx: &mut TestAppContext, state: &Path, root: Option<&Path>) -> BatchSender {
    // Plugins load on threads of their own.
    cx.executor().allow_parking();
    let user = state.join("settings.toml");
    if !user.exists() {
        fs::write(&user, "").unwrap();
    }
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .commands(config::register_commands)
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, state.join("extensions"));
    let (plugin_state, cache) = (state.join("state"), state.join("cache"));
    let root = root.map(Path::to_owned);
    cx.update(|cx| {
        trust::init(cx);
        fe_workbench::set_plugin_dirs(cx, plugin_state, cache);
        kiln::init(cx, config);
        if let Some(root) = root {
            workspace_extensions::follow(Some(root), cx);
        }
    });
    cx.run_until_parked();
    sender
}

/// A workbench with no folder, active and focused, in a window of its own.
fn window(cx: &mut TestAppContext) -> &mut VisualTestContext {
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
    vcx
}

fn draw(vcx: &mut VisualTestContext) {
    for _ in 0..2 {
        vcx.run_until_parked();
        vcx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// Draws until `done`: plugins load and stop on threads of their own.
fn until(vcx: &mut VisualTestContext, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        draw(vcx);
        if vcx.update(|_, cx| done(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The plugin's state, if the host has it.
fn plugin_state(cx: &App) -> Option<PluginState> {
    (fe_workbench::plugin_states(cx).into_iter())
        .find(|info| info.id == PLUGIN)
        .map(|info| info.state)
}

fn is(expected: PluginState) -> impl Fn(&App) -> bool {
    move |cx| plugin_state(cx) == Some(expected.clone())
}

/// The plugin's commands the palette lists in the window's focus.
fn listed(vcx: &mut VisualTestContext) -> Vec<String> {
    draw(vcx);
    vcx.update(|window, cx| {
        let stack = kiln::platform::keys::context_stack(&window.context_stack(), cx);
        (kiln::shell::command_palette::commands_in(&stack, cx).into_iter())
            .map(|entry| entry.id.to_string())
            .filter(|id| id.starts_with(PLUGIN))
            .collect()
    })
}

/// Writes the user's settings and sends the batch the watcher would.
fn save_settings(state: &Path, text: &str, sender: &BatchSender, vcx: &mut VisualTestContext) {
    let user = state.join("settings.toml");
    fs::write(&user, text).unwrap();
    sender.send(vec![user]);
    draw(vcx);
}

#[gpui::test]
fn preview_off_loads_no_plugin(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    install(&state.path().join("extensions"));
    grant(state.path());
    let _sender = load(cx, state.path(), None);
    let vcx = window(cx);
    std::thread::sleep(Duration::from_millis(100));
    draw(vcx);

    // Granted, it's still neither compiled nor loaded, nor asked about.
    assert_eq!(
        vcx.update(|_, cx| plugin_state(cx)),
        Some(PluginState::PreviewOff)
    );
    assert_eq!(cached(state.path()), 0);
    assert!(!vcx.has_pending_prompt());
    assert_eq!(
        vcx.update(|_, cx| fe_workbench::pending_plugin_grants(cx)),
        []
    );
    assert_eq!(listed(vcx), Vec::<String>::new());
    // Its manifest loads quietly.
    assert_eq!(vcx.update(|_, cx| diagnostics(cx).to_vec()), []);
    let info = vcx
        .update(|_, cx| fe_workbench::plugin_states(cx))
        .remove(0);
    assert_eq!(info.requested, requested());
    assert_eq!(info.granted, requested());
}

#[gpui::test]
fn the_preview_setting_applies_live(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    install(&state.path().join("extensions"));
    grant(state.path());
    let sender = load(cx, state.path(), None);
    let vcx = window(cx);
    assert_eq!(
        vcx.update(|_, cx| plugin_state(cx)),
        Some(PluginState::PreviewOff)
    );
    let granted = fs::read_to_string(state.path().join("state").join(GRANTS_FILE)).unwrap();

    // On: the granted plugin loads, with no restart and no prompt.
    let on = "[extensions]\nplugins_preview = true\n";
    save_settings(state.path(), on, &sender, vcx);
    until(vcx, "the plugin to load", is(PluginState::Running));
    assert!(!vcx.has_pending_prompt());
    assert_eq!(cached(state.path()), 1);

    // Off: it unloads once its shutdown has run, its commands withdrawn and
    // its grant kept.
    save_settings(state.path(), "", &sender, vcx);
    until(vcx, "the plugin to unload", is(PluginState::PreviewOff));
    assert_eq!(listed(vcx), Vec::<String>::new());
    let record = store(state.path()).get(PLUGIN).unwrap().unwrap();
    assert_eq!(record.version, Some((1, 0, 0)));
    assert_eq!(record.granted, requested());
    assert_eq!(
        fs::read_to_string(state.path().join("state").join(GRANTS_FILE)).unwrap(),
        granted
    );
    assert!(!vcx.has_pending_prompt());

    // On again: it loads without asking.
    save_settings(state.path(), on, &sender, vcx);
    until(vcx, "the plugin to load again", is(PluginState::Running));
    assert!(!vcx.has_pending_prompt());
    assert_eq!(
        vcx.update(|_, cx| fe_workbench::pending_plugin_grants(cx)),
        []
    );
}

#[gpui::test]
fn a_workspace_extension_cannot_load_a_plugin(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("project");
    let workspace_dir = install(&root.join(".tenmoku/extensions"));
    let root = canonical(&root);
    // Granted as if to a copy in the user's directory, and the preview on.
    grant(state.path());
    fs::write(
        state.path().join("settings.toml"),
        "[extensions]\nplugins_preview = true\n",
    )
    .unwrap();
    let _sender = load(cx, state.path(), Some(&root));
    cx.update(|cx| trust::decide(&root, true, cx));
    let vcx = window(cx);
    until(vcx, "the workspace's extension to load", |cx| {
        kiln::extensions::loaded(cx)
            .iter()
            .any(|manifest| manifest.dir == workspace_dir || manifest.id == PLUGIN)
    });
    std::thread::sleep(Duration::from_millis(100));
    draw(vcx);

    // Its plugin is dropped with a warning: the grant doesn't cover it, and
    // nothing is compiled, loaded or asked about.
    let found = vcx.update(|_, cx| diagnostics(cx).to_vec());
    assert!(
        found
            .iter()
            .any(|diagnostic| diagnostic.message.contains("can't contribute plugin")),
        "{found:#?}"
    );
    assert_eq!(vcx.update(|_, cx| plugin_state(cx)), None);
    assert_eq!(cached(state.path()), 0);
    assert!(!vcx.has_pending_prompt());
    assert_eq!(listed(vcx), Vec::<String>::new());
}
