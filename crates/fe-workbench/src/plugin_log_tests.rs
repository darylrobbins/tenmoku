//! A plugin's own log: what its `log` import writes, with level and time,
//! and the host's diagnostics of it, kept per plugin, the last 1,000; and
//! Extensions: Show Plugin Log, a read-only tab that follows it. The
//! plugin is kiln-plugin's commands fixture.

use std::{
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime},
};

use fe_command::{ContextFrame, ContextStack, Invocation, Value, capability::Capability};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Action as _, App, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{ApiVersion, extension::load_manifest},
    platform::config::batch_channel,
};
use kiln_plugin::{
    grants::GrantStore,
    manifest::{ExtensionOrigin, PluginManifest},
};
use kiln_shell::PaneItem as _;

use super::{LogLine, MAX_LINES, PluginLogs, lines, push};
use crate::{
    GRANTS_FILE, Launch, PluginState, Workbench,
    extension_install::check_api,
    plugin_host::{Found, set_plugins, set_preview},
    plugin_kind, plugin_states, set_plugin_dirs,
    tests::open_root,
    unload_plugin,
};

/// Registers `acme.tool.hello`, whose `run-command` logs its id and args.
const COMMANDS: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/commands.wat");

/// Exports `init` and `shutdown`, and does nothing.
const EMPTY: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/empty.wat");

const TOOL: &str = "acme.tool";

/// Writes plugin [`TOOL`] into `dir`, its component `wat`, asking for
/// `commands`.
fn write_plugin(dir: &Path, wat: &str) {
    write_plugin_as(dir, TOOL, wat);
}

/// Writes plugin `id` into `dir`, its component `wat`, asking for
/// `commands`.
fn write_plugin_as(dir: &Path, id: &str, wat: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("plugin.wasm"), wat).unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"{id}\"\nname = \"Tool\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.1\"\n\n\
             [contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [\"commands\"]\n"
        ),
    )
    .unwrap();
}

/// The plugin in `dir`, as the host is handed it.
fn found(dir: &Path) -> Found {
    let (manifest, diagnostics) = load_manifest(dir, &check_api(Vec::new()));
    let manifest = manifest.unwrap_or_else(|| panic!("{diagnostics:#?}"));
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User).unwrap();
    Found::new(&manifest, Ok(plugin.unwrap()))
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

fn logged(cx: &mut VisualTestContext) -> Vec<LogLine> {
    cx.update(|_, cx| lines(TOOL, cx))
}

fn dispatch(cx: &mut VisualTestContext, command: &str, args: Value, frames: &[&str]) {
    let context = (frames.iter()).fold(ContextStack::new(), |stack, frame| {
        stack.with(ContextFrame::new(*frame))
    });
    let invocation = Invocation::new(command.parse().unwrap())
        .args(args)
        .context(context);
    cx.update(|window, cx| window.dispatch_action(RunCommand(invocation).boxed_clone(), cx));
}

#[gpui::test]
fn log_lines_reach_the_plugin_log(cx: &mut TestAppContext) {
    // Kept per plugin, the last 1,000, each with its time and level.
    let shared = PluginLogs::default();
    let before = SystemTime::now();
    for n in 0..MAX_LINES + 5 {
        shared.push("a", log::Level::Warn, format!("line {n}"));
    }
    shared.push("b", log::Level::Debug, "only");
    let kept = shared.lines("a");
    assert_eq!(kept.len(), MAX_LINES);
    assert_eq!(kept[0].message, "line 5");
    assert_eq!(
        kept[MAX_LINES - 1].message,
        format!("line {}", MAX_LINES + 4)
    );
    assert!(kept.iter().all(|line| line.level == log::Level::Warn));
    assert!((kept.iter()).all(|line| line.time >= before && line.time <= SystemTime::now()));
    let text = kept[0].to_string();
    assert!(text.ends_with(" WARN  line 5"), "{text}");
    assert_eq!(shared.lines("b").len(), 1);
    assert_eq!(shared.lines("c"), []);

    // A running plugin's log import, and what the host says of it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let cache = dir.path().join("cache");
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_plugin_dirs(cx, state.clone(), cache);
        set_preview(cx, true);
    });
    let grants = [Capability::parse("commands").unwrap()]
        .into_iter()
        .collect();
    (GrantStore::new(state.join(GRANTS_FILE)))
        .grant(TOOL, (1, 0, 0), &grants, &[], false)
        .unwrap();
    let plugin_dir = dir.path().join("extensions/tool");
    write_plugin(&plugin_dir, COMMANDS);
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
    let tool = found(&plugin_dir);
    vcx.update(|_, cx| set_plugins(cx, vec![tool]));
    until(vcx, "the plugin to run", |cx| {
        (plugin_states(cx).iter()).any(|info| info.state == PluginState::Running)
    });
    dispatch(
        vcx,
        "acme.tool.hello",
        serde_json::json!({"a": 1}),
        &["workbench", "editor"],
    );
    until(vcx, "the plugin to log", |cx| lines(TOOL, cx).len() == 3);
    let shown: Vec<(log::Level, String)> = (logged(vcx).into_iter())
        .map(|line| (line.level, line.message))
        .collect();
    assert_eq!(
        shown,
        [
            (log::Level::Info, "loaded 1.0.0".to_owned()),
            (log::Level::Info, "acme.tool.hello".to_owned()),
            (log::Level::Info, "{\"a\":1}".to_owned()),
        ]
    );

    // Unloaded, then failing to load.
    vcx.update(|_, cx| unload_plugin(TOOL, cx));
    until(vcx, "the unload", |cx| lines(TOOL, cx).len() == 4);
    assert_eq!(logged(vcx)[3].message, "shut down");
    fs::write(plugin_dir.join("plugin.wasm"), "(component broken").unwrap();
    let broken = found(&plugin_dir);
    vcx.update(|_, cx| set_plugins(cx, vec![broken]));
    until(vcx, "the failed load", |cx| lines(TOOL, cx).len() == 5);
    let failed = logged(vcx).remove(4);
    assert_eq!(failed.level, log::Level::Error);
    assert!(failed.message.starts_with("failed to load: "), "{failed:?}");

    // A shutdown that traps.
    const QUIT: &str = "acme.quit";
    let trapping = EMPTY.replace(
        "(func (export \"shutdown\"))",
        "(func (export \"shutdown\") unreachable)",
    );
    let quit_dir = dir.path().join("extensions/quit");
    write_plugin_as(&quit_dir, QUIT, &trapping);
    (GrantStore::new(state.join(GRANTS_FILE)))
        .grant(QUIT, (1, 0, 0), &grants, &[], false)
        .unwrap();
    let quit = found(&quit_dir);
    vcx.update(|_, cx| set_plugins(cx, vec![quit.clone()]));
    until(vcx, "the trapping plugin to run", |cx| {
        lines(QUIT, cx).len() == 1
    });
    vcx.update(|_, cx| unload_plugin(QUIT, cx));
    until(vcx, "its shutdown", |cx| lines(QUIT, cx).len() == 3);
    let quitting = vcx.update(|_, cx| lines(QUIT, cx));
    assert_eq!(quitting[1].level, log::Level::Error);
    let message = &quitting[1].message;
    assert!(
        message.starts_with("its shutdown failed: trapped"),
        "{message}"
    );
    assert_eq!(quitting[2].message, "shut down");

    // An answer to its prompt that can't be recorded.
    const ASK: &str = "acme.ask";
    let ask_dir = dir.path().join("extensions/ask");
    write_plugin_as(&ask_dir, ASK, EMPTY);
    let ask = found(&ask_dir);
    vcx.update(|_, cx| set_plugins(cx, vec![quit, ask]));
    draw(vcx);
    assert!(vcx.has_pending_prompt());
    // Its grants can't be written now.
    fs::rename(&state, dir.path().join("state-aside")).unwrap();
    fs::write(&state, "").unwrap();
    vcx.simulate_prompt_answer("Allow");
    until(vcx, "the answer to fail", |cx| !lines(ASK, cx).is_empty());
    let asked = vcx.update(|_, cx| lines(ASK, cx)).remove(0);
    assert_eq!(asked.level, log::Level::Error);
    let message = &asked.message;
    assert!(
        message.starts_with("can't record the answer to its prompt: "),
        "{message}"
    );
}

#[gpui::test]
fn show_plugin_log_opens_a_following_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extensions = dir.path().join("extensions");
    write_plugin(&extensions.join("tool"), COMMANDS);
    let (_sender, batches) = batch_channel();
    let plugin = plugin_kind();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .extensions(
            extensions,
            &fe_config::paths::app_id().short,
            ApiVersion { major: 1, minor: 1 },
        )
        .extension_kind(plugin.name, plugin.schema)
        .watch_with(batches);
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, config), []);
        push(TOOL, log::Level::Info, "loaded 1.0.0", cx);
        push(TOOL, log::Level::Warn, "careful", cx);
    });
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let (workbench, vcx) = open_root(cx, &root);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    dispatch(vcx, "workbench.new_file", Value::Null, &["workbench"]);
    draw(vcx);

    // The command lists the plugins; Enter on one opens its log, shown over
    // the editor open.
    dispatch(
        vcx,
        "workbench.show_plugin_log",
        Value::Null,
        &["workbench"],
    );
    draw(vcx);
    vcx.simulate_input("tool");
    draw(vcx);
    vcx.simulate_keystrokes("enter");
    draw(vcx);
    let editor = workbench.read_with(vcx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("the log's tab");
    let text = |vcx: &mut VisualTestContext| {
        editor.read_with(vcx, |editor, _| editor.model().buffer().text().to_string())
    };
    let shown = text(vcx);
    let expected: String = (logged(vcx).iter())
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(shown, expected);
    assert!(shown.contains(" WARN  careful\n"), "{shown}");
    editor.read_with(vcx, |editor, cx| {
        assert!(editor.is_read_only());
        assert_eq!(editor.tab_title(cx), "Tool Log");
    });

    // A line written while it's open is added at the end.
    vcx.update(|_, cx| push(TOOL, log::Level::Error, "stopped: trapped", cx));
    draw(vcx);
    let shown = text(vcx);
    assert!(shown.starts_with(&expected), "{shown}");
    let added = &shown[expected.len()..];
    assert!(added.ends_with(" ERROR stopped: trapped\n"), "{added}");
    assert!(!editor.read_with(vcx, |editor, _| editor.model().is_dirty()));

    // Split, the first view closed, the split follows on.
    dispatch(vcx, "pane.split_right", Value::Null, &["workbench"]);
    draw(vcx);
    dispatch(vcx, "pane.focus_left", Value::Null, &["workbench"]);
    draw(vcx);
    dispatch(vcx, "workbench.close_tab", Value::Null, &["workbench"]);
    draw(vcx);
    let logs_open = |vcx: &mut VisualTestContext| {
        workbench.read_with(vcx, |workbench, cx| {
            (workbench.open_editors(cx).into_iter())
                .filter(|editor| editor.read(cx).tab_title(cx) == "Tool Log")
                .collect::<Vec<_>>()
        })
    };
    let split = logs_open(vcx);
    assert_eq!(split.len(), 1);
    assert_ne!(split[0], editor);
    let split = split[0].clone();
    let text = |vcx: &mut VisualTestContext| {
        split.read_with(vcx, |editor, _| editor.model().buffer().text().to_string())
    };
    vcx.update(|_, cx| push(TOOL, log::Level::Info, "after the split", cx));
    draw(vcx);
    assert!(text(vcx).ends_with(" INFO  after the split\n"));

    // Past 1,000 lines, the oldest go from the tab as from the log.
    vcx.update(|_, cx| {
        for n in 0..MAX_LINES {
            push(TOOL, log::Level::Info, format!("line {n}"), cx);
        }
    });
    draw(vcx);
    let kept: String = (logged(vcx).iter())
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(text(vcx), kept);
    assert_eq!(text(vcx).lines().count(), MAX_LINES);
}
