//! Debug sessions in a headless workbench (`debug_session.rs`, debugger
//! card D7) against fe-dap's fake adapter, which the host's launcher
//! starts in-process: trust, pre-launch builds, the debuggee's tab, stops,
//! breakpoints sent as they change, settings and the ptrace toast. The
//! terminal panel's sessions are fake, so no process starts and each test
//! emits the exits it wants.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::SettingsEvent;
use fe_dap::{
    AdapterSpec, AdapterTransport, EndReason, HostOs, ManualClock, Message, SessionError,
    SessionState, SystemClock,
    fake::{FakeAdapter, FakeLauncher, Fault, FaultKind, Line, Script, Startup, Trigger},
};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_terminal_view::TerminalViewEvent;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;
use serde_json::Value;

use crate::{
    Launch, TerminalPanel, Workbench,
    debug_breakpoints::BreakpointStore,
    debug_session::{DebugHost, Debugger, DebuggerEvent, launch, picker::ShownRow, set_debug_host},
    launch_defs::LaunchConfig,
    task_commands::TestFiles,
    task_defs::{ProjectFiles, Source, Task, TaskCommand},
    task_run::{ExitHeard, FakeTaskOutput, TaskExit},
    terminal_panel::Debuggee,
    workbench::toast::Level,
};

/// The project's files, read from disk.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// The program the fake debugs: `src/main.rs`'s five lines.
const MAIN: &str = "let x = 1;\nlet point = 2;\nlet y = add(x, 2);\na + b\nprintln!(y);\n";

/// The launch file: `main` runs at once, `built` after the task `build`.
const LAUNCH: &str = r#"
[[launch]]
name = "main"
adapter = "fake"
program = "${workspaceFolder}/target/debug/main"

[[launch]]
name = "built"
adapter = "fake"
program = "${workspaceFolder}/target/debug/main"
pre_launch_task = "build"
"#;

const TASKS: &str = r#"
[[task]]
label = "build"
command = ["cargo", "build"]
"#;

/// A workbench on a project the fake debugs, with the fake as the host's
/// adapter `fake`.
struct Debug<'a> {
    _dir: tempfile::TempDir,
    _link_dir: Option<tempfile::TempDir>,
    root: PathBuf,
    workbench: Entity<Workbench>,
    launcher: Arc<FakeLauncher>,
    /// How many times the ptrace scope was read.
    scope_reads: Arc<AtomicUsize>,
    cx: &'a mut VisualTestContext,
}

/// The fake's script for the project at `root`: its program is
/// `src/main.rs`.
fn script(root: &Path) -> Script {
    let mut script = Script::default();
    script.program.path = main_path(root).to_string_lossy().into_owned();
    script
}

fn main_path(root: &Path) -> PathBuf {
    root.join("src").join("main.rs")
}

/// `script`, its program spinning on its last line until paused: a
/// session that runs until it's stopped.
fn spinning(mut script: Script) -> Script {
    let last = script.program.lines.len() - 1;
    script.program.lines[last] = Line {
        spin: true,
        ..script.program.lines[last].clone()
    };
    script
}

/// The workbench, its root not trusted, on a project with `files` besides
/// `src/main.rs`, the fake following `script` (made from the root).
fn untrusted<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    script: impl FnOnce(&Path) -> Script,
) -> Debug<'a> {
    untrusted_with(cx, files, script, Setup::default())
}

/// What a test changes of the app the workbench opens in.
#[derive(Default)]
struct Setup {
    /// The sessions' clock, else real time.
    clock: Option<Arc<ManualClock>>,
    /// The user's `settings.toml`, loaded as the app starts, else none.
    user_settings: Option<&'static str>,
    /// Opens the workbench at a symlink to the project.
    symlinked: bool,
}

fn untrusted_with<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    script: impl FnOnce(&Path) -> Script,
    setup: Setup,
) -> Debug<'a> {
    // The session's thread wakes the workbench's event loop.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(main_path(&root), MAIN).unwrap();
    for (name, text) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // Its Rust file would start a real rust-analyzer that outlives the test.
    let settings = fe_config::project_settings_path(&root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
    let launcher = Arc::new(FakeLauncher::new(script(&root)));
    let scope_reads = Arc::new(AtomicUsize::new(0));
    let host = DebugHost {
        launcher: Some(launcher.clone()),
        clock: match setup.clock {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        },
        specs: vec![AdapterSpec {
            types: vec!["fake".into()],
            languages: vec!["rust".into()],
            transport: AdapterTransport::Port(1),
            ..AdapterSpec::new("fake", "fake")
        }],
        ptrace_scope: Arc::new({
            let reads = scope_reads.clone();
            move || {
                reads.fetch_add(1, Ordering::SeqCst);
                Some("2\n".to_owned())
            }
        }),
        login: None,
        os: HostOs::Linux,
    };
    if let Some(user) = setup.user_settings {
        let path = dir.path().join("settings.toml");
        fs::write(&path, user).unwrap();
        let store = fe_config::SettingsStore::load(Some(path), None);
        let config =
            kiln::Config::new("dev.fastedit.test", "fastedit").settings_store(store.into_inner());
        cx.update(|cx| assert_eq!(kiln::init(cx, config), []));
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_debug_host(cx, host);
        cx.set_global(TestFiles(Arc::new(Disk)));
    });
    // The link in a folder of its own, so the project holds no loop.
    let link_dir = setup.symlinked.then(|| tempfile::tempdir().unwrap());
    let opened = match &link_dir {
        #[cfg(unix)]
        Some(link_dir) => {
            let link = link_dir.path().join("project");
            std::os::unix::fs::symlink(&root, &link).unwrap();
            link
        }
        _ => root.clone(),
    };
    let launch = Launch {
        root: Some(opened),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    let mut debug = Debug {
        _dir: dir,
        _link_dir: link_dir,
        root,
        workbench,
        launcher,
        scope_reads,
        cx,
    };
    debug.draw();
    debug
}

/// As [`untrusted`], with the root trusted.
fn trusted<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    script: impl FnOnce(&Path) -> Script,
) -> Debug<'a> {
    let mut debug = untrusted(cx, files, script);
    debug.set_trust(true);
    debug
}

impl Debug<'_> {
    fn draw(&mut self) {
        for _ in 0..2 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    /// Draws until `done` holds. Panics after 30 s, naming `what`.
    fn until(&mut self, what: &str, done: impl Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.draw();
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn set_trust(&mut self, trusted: bool) {
        let workbench = self.workbench.clone();
        workbench.update(self.cx, |workbench, cx| {
            workbench.set_root_trust(trusted, cx)
        });
        self.draw();
    }

    /// Runs workbench command `id` as a menu item would.
    fn run(&mut self, id: &str) {
        let run = RunCommand(
            Invocation::of(id)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        self.draw();
    }

    /// Starts the configuration `name`, as `debug.rerun_last` and a
    /// debuggee tab's rerun do.
    fn start(&mut self, name: &str) {
        let workbench = self.workbench.clone();
        let name = name.to_owned();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.rerun_debug_named(name, window, cx)
            });
        });
        self.draw();
    }

    fn debugger(&mut self) -> Entity<Debugger> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.debugger(cx)))
            .expect("the window's debugger")
    }

    fn state(&mut self) -> Option<SessionState> {
        let debugger = self.debugger();
        debugger.read_with(self.cx, |debugger, _| debugger.state())
    }

    fn wait_state(&mut self, what: &str, done: impl Fn(&SessionState) -> bool) {
        self.until(what, |debug| debug.state().as_ref().is_some_and(&done));
    }

    /// The `n`th fake launched, from 0, once it has been.
    fn fake(&mut self, n: usize) -> FakeAdapter {
        let launcher = self.launcher.clone();
        self.until(&format!("launch {n}"), |_| launcher.launched().len() > n);
        launcher.wait_for_launch(n)
    }

    fn store(&mut self) -> Entity<BreakpointStore> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.breakpoint_store(cx)))
            .expect("rooted")
    }

    fn toggle(&mut self, path: &Path, row: u32) {
        let store = self.store();
        let path = path.to_owned();
        store.update(self.cx, |store, cx| store.toggle(&path, row, cx));
        self.draw();
    }

    fn terminal(&mut self) -> Entity<TerminalPanel> {
        self.workbench
            .read_with(self.cx, |workbench, _| workbench.terminal().clone())
    }

    /// The tabs the last frame listed.
    fn tabs(&mut self) -> Vec<String> {
        self.draw();
        let terminal = self.terminal();
        terminal.read_with(self.cx, |terminal, _| {
            (terminal.drawn_tabs().iter())
                .map(|(label, _)| label.to_string())
                .collect()
        })
    }

    /// What each terminal session was asked to run, in start order.
    fn started(&mut self) -> Vec<Option<(String, Vec<String>)>> {
        let terminal = self.terminal();
        terminal.read_with(self.cx, |terminal, _| {
            (terminal.started_options.iter())
                .map(|options| options.shell.clone())
                .collect()
        })
    }

    /// Ends the active terminal tab's fake session with `code`.
    fn exit_active(&mut self, code: Option<i32>) {
        let terminal = self.terminal();
        let view = terminal
            .read_with(self.cx, |terminal, _| terminal.view().cloned())
            .expect("a tab");
        view.update(self.cx, |_, cx| cx.emit(TerminalViewEvent::Exited(code)));
        self.draw();
    }

    /// Each toast showing: level, message and its action's title.
    fn toasts(&mut self) -> Vec<(Level, String, Option<String>)> {
        let layer = self
            .workbench
            .read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| {
                    let toast = &entry.toast;
                    let action = toast.action.as_ref().map(|(title, _)| title.clone());
                    (toast.level, toast.message.clone(), action)
                })
                .collect()
        })
    }

    fn active_editor(&mut self) -> Option<Entity<EditorView>> {
        self.workbench.read_with(self.cx, |workbench, cx| {
            workbench.active_editor(cx).cloned()
        })
    }

    /// The active editor's file, and its caret's row.
    fn caret(&mut self) -> Option<(PathBuf, u32)> {
        let editor = self.active_editor()?;
        editor.read_with(self.cx, |editor, _| {
            let model = editor.model();
            let row = (model.text())
                .offset_to_point(model.primary_selection().head())
                .row;
            Some((model.path()?.to_owned(), row))
        })
    }

    /// What the window's live region says now.
    fn said(&mut self) -> String {
        self.draw();
        let nodes = self.cx.update(|window, cx| fe_ui::nodes(window, cx));
        let status = (nodes.into_iter().rev())
            .find(|node| node.role == fe_ui::Role::Status)
            .expect("the announcer's Status node");
        status
            .states
            .value
            .map(|v| v.to_string())
            .unwrap_or_default()
    }

    /// The session's protocol log, as `debug.show_log` shows it.
    fn log(&mut self) -> String {
        let debugger = self.debugger();
        debugger.read_with(self.cx, |debugger, _| {
            (debugger.log().iter())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    fn set_log_payloads(&mut self, on: bool) {
        let mut settings = fe_config::Settings::default();
        settings.debug.log_payloads = on;
        self.reload(settings);
    }

    /// Applies `settings`, as a reload of the user's settings does.
    fn reload(&mut self, settings: fe_config::Settings) {
        let event = SettingsEvent {
            settings: Arc::new(settings),
            previous: Arc::new(fe_config::Settings::default()),
            diagnostics: Vec::new(),
        };
        let workbench = self.workbench.clone();
        workbench.update(self.cx, |workbench, cx| {
            workbench.reload_settings(&event, cx)
        });
        self.draw();
    }
}

/// The arguments of each request `command` the fake got.
fn requests(fake: &FakeAdapter, command: &str) -> Vec<Value> {
    (fake.received().into_iter())
        .filter_map(|message| match message {
            Message::Request(request) if request.command == command => {
                Some(request.arguments.unwrap_or(Value::Null))
            }
            _ => None,
        })
        .collect()
}

/// The lines of each `setBreakpoints` the fake got for `path`.
fn sent_lines(fake: &FakeAdapter, path: &Path) -> Vec<Vec<i64>> {
    (requests(fake, "setBreakpoints").into_iter())
        .filter(|arguments| arguments["source"]["path"] == path.to_string_lossy().as_ref())
        .map(|arguments| {
            (arguments["breakpoints"].as_array().into_iter().flatten())
                .filter_map(|breakpoint| breakpoint["line"].as_i64())
                .collect()
        })
        .collect()
}

fn launch_file() -> [(&'static str, &'static str); 2] {
    [
        (".tenmoku/launch.toml", LAUNCH),
        (".tenmoku/tasks.toml", TASKS),
    ]
}

/// C1 (§8): an untrusted folder's configurations are listed, and picking
/// one starts nothing but the toast asking for trust, with its button.
#[gpui::test]
fn an_untrusted_root_lists_configurations_and_refuses_to_start_one(cx: &mut TestAppContext) {
    let mut debug = untrusted(cx, &launch_file(), script);
    debug.run("debug.start");
    let workbench = debug.workbench.clone();
    let picker = debug
        .cx
        .update(|_, cx| workbench.read(cx).debug_picker(cx))
        .expect("the configuration picker shows");
    debug.until("the list", |debug| {
        picker.read_with(debug.cx, |picker, _| picker.delegate.loaded())
    });
    let shown = picker.read_with(debug.cx, |picker, _| picker.delegate.shown());
    let names: Vec<&str> = (shown.iter())
        .filter_map(|row| match row {
            ShownRow::Config(name, _) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["main", "built"], "{shown:?}");

    picker.update_in(debug.cx, |picker, window, cx| picker.confirm(0, window, cx));
    debug.draw();
    let untrusted = tr!("workbench-debug-untrusted").to_string();
    let trust = tr!("workbench-debug-trust-folder").to_string();
    assert_eq!(
        debug.toasts(),
        [(Level::Warning, untrusted.clone(), Some(trust.clone()))]
    );
    assert_eq!(untrusted, "Trust this folder to debug its programs");
    assert_eq!(debug.launcher.launched().len(), 0, "no adapter started");
    assert_eq!(debug.state(), None);

    // Every path holds trust: a rerun too.
    debug.start("main");
    thread::sleep(Duration::from_millis(50));
    debug.draw();
    assert_eq!(debug.launcher.launched().len(), 0, "no adapter started");

    // The trust prompt says what trust lets debugging do.
    let detail = tr!("workbench-extensions-trust-detail", folder = "/project").to_string();
    assert!(
        detail.contains("and run its debug configurations"),
        "{detail}"
    );
}

/// C2 (§7): a pre-launch task that fails starts no session, and says so.
#[gpui::test]
fn a_failing_pre_launch_task_starts_no_session(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), script);
    debug.start("built");
    assert_eq!(debug.tabs(), ["build"], "the task runs in its tab");
    assert_eq!(
        debug.started().last().cloned().flatten(),
        Some(("cargo".to_owned(), vec!["build".to_owned()]))
    );
    debug.exit_active(Some(101));
    thread::sleep(Duration::from_millis(50));
    debug.draw();
    assert_eq!(debug.launcher.launched().len(), 0, "no session started");
    assert_eq!(debug.state(), None);
    let failed = tr!("workbench-debug-pre-launch-failed", label = "build").to_string();
    assert!(
        (debug.toasts().iter())
            .any(|(level, message, _)| *level == Level::Error && *message == failed),
        "{:?}",
        debug.toasts()
    );

    // Once it passes, the session starts.
    debug.start("built");
    debug.exit_active(Some(0));
    let fake = debug.fake(0);
    let launch = requests(&fake, "launch");
    assert_eq!(
        launch[0]["program"],
        debug
            .root
            .join("target/debug/main")
            .to_string_lossy()
            .as_ref()
    );
}

/// C2 (§7): a detected Cargo configuration builds with cargo's JSON
/// messages, and debugs the executable its `compiler-artifact` message
/// names, read through the task runner's exit seam.
#[gpui::test]
fn a_detected_cargo_configuration_debugs_the_artifact_cargo_built(cx: &mut TestAppContext) {
    let cargo = "[package]\nname = \"tool\"\nversion = \"0.1.0\"\n";
    let mut debug = trusted(cx, &[("Cargo.toml", cargo)], script);
    let name = tr!("workbench-debug-launch-detected-bin", bin = "tool").to_string();
    debug.start(&name);
    let argv = [
        "cargo",
        "build",
        "--bin",
        "tool",
        "--message-format=json-render-diagnostics",
    ];
    assert_eq!(
        debug.started().last().cloned().flatten(),
        Some((
            argv[0].to_owned(),
            argv[1..].iter().map(|arg| (*arg).to_owned()).collect()
        ))
    );
    let built = debug.root.join("elsewhere/target/debug/tool");
    let artifact = serde_json::json!({
        "reason": "compiler-artifact",
        "target": {"name": "tool", "kind": ["bin"]},
        "executable": built.to_string_lossy(),
    });
    let library = serde_json::json!({
        "reason": "compiler-artifact",
        "target": {"name": "tool", "kind": ["lib"]},
        "executable": null,
    });
    let output = format!(
        "   Compiling tool v0.1.0\n{library}\n{artifact}\n{}\n",
        r#"{"reason":"build-finished","success":true}"#
    );
    debug
        .cx
        .update(|_, cx| cx.set_global(FakeTaskOutput(output)));
    debug.exit_active(Some(0));
    let fake = debug.fake(0);
    debug.until("the launch request", |_| {
        !requests(&fake, "launch").is_empty()
    });
    let launch = requests(&fake, "launch");
    assert_eq!(launch[0]["program"], built.to_string_lossy().as_ref());
    assert_eq!(launch[0]["cwd"], debug.root.to_string_lossy().as_ref());
}

/// C3 (§5): a stop opens the stopped frame's file with the caret on its
/// row, and says "Stopped at line N".
#[gpui::test]
fn a_stop_opens_the_file_at_the_stopped_row_and_announces_it(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.start("main");
    let fake = debug.fake(0);
    debug.until("the breakpoints to be sent", |_| {
        !sent_lines(&fake, &main).is_empty()
    });
    assert_eq!(sent_lines(&fake, &main), [vec![3]], "sent as it starts");
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    debug.until("the file to open", |debug| {
        debug.caret() == Some((main.clone(), 2))
    });
    assert_eq!(
        debug.said(),
        tr!("workbench-debug-stopped-at-line", line = 3).to_string()
    );
    assert_eq!(debug.said(), "Stopped at line 3");
    let debugger = debug.debugger();
    let (row, path) = debugger.read_with(debug.cx, |debugger, _| {
        let stop = debugger.stop_location().expect("stopped");
        (stop.row, stop.path.clone())
    });
    assert_eq!((row, path), (2, Some(main.clone())));
    let verified = debugger.read_with(debug.cx, |debugger, _| {
        (debugger.verification(&main).iter())
            .map(|verification| (verification.line, verification.verified))
            .collect::<Vec<_>>()
    });
    assert_eq!(verified, [(3, true)]);

    // `debug.start` while stopped continues.
    debug.run("debug.start");
    debug.wait_state("the end", SessionState::is_ended);
    assert!(fake.commands().contains(&"continue".to_owned()));
}

/// C4 (§8): untrusting the folder stops its running session.
#[gpui::test]
fn untrusting_stops_a_running_session(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| spinning(script(root)));
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("running", |state| *state == SessionState::Running);
    debug.set_trust(false);
    debug.wait_state("the end", SessionState::is_ended);
    assert!(
        (fake.commands().iter()).any(|command| command == "terminate" || command == "disconnect"),
        "{:?}",
        fake.commands()
    );
    assert_eq!(debug.said(), "Session ended");
}

/// C5 (§5, §7): the debuggee's tab isn't a task: `tasks.rerun_last` still
/// reruns the last real task, and the tab's rerun starts a fresh session,
/// whose new `runInTerminal` it runs, never the old argv.
#[gpui::test]
fn a_debuggee_tab_never_becomes_the_last_task(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        run_in_terminal: true,
        ..script(root)
    });
    let task = Task {
        label: "check".to_owned(),
        command: TaskCommand::Argv(vec!["cargo".to_owned(), "check".to_owned()]),
        cwd: debug.root.clone(),
        env: Default::default(),
        group: None,
        default: false,
        source: Source::File,
        problem_matcher: None,
    };
    let (workbench, real) = (debug.workbench.clone(), task.clone());
    debug.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.run_task(real, window, cx));
    });
    debug.draw();
    debug.exit_active(Some(0));

    let first = debug.launcher.clone();
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("the end", SessionState::is_ended);
    let program = main_path(&debug.root).to_string_lossy().into_owned();
    assert_eq!(
        debug.started().last().cloned().flatten(),
        Some((program.clone(), Vec::new())),
        "the adapter's argv, run directly"
    );
    assert!((fake.sent().iter()).any(
        |message| matches!(message, Message::Request(request) if request.command == "runInTerminal")
    ));
    debug.exit_active(Some(0));
    assert_eq!(debug.tabs(), ["check ✓", "main ✓"]);
    let last = debug
        .workbench
        .read_with(debug.cx, |workbench, cx| workbench.last_task(cx));
    assert_eq!(last.map(|task| task.label), Some("check".to_owned()));

    debug.run("tasks.rerun_last");
    assert_eq!(
        debug.started().last().cloned().flatten(),
        Some(("cargo".to_owned(), vec!["check".to_owned()])),
        "the real task reruns"
    );
    debug.exit_active(Some(0));

    // The tab's rerun: a fresh session, whose adapter asks for a new argv.
    let mut next = script(&debug.root);
    next.run_in_terminal = true;
    next.program.path = debug
        .root
        .join("src/other.rs")
        .to_string_lossy()
        .into_owned();
    first.set_script(next.clone());
    // The tab passed, and its button reruns it: the debuggee's tab active,
    // a click on the button its accessible name names.
    let terminal = debug.terminal();
    debug.cx.update(|window, cx| {
        terminal.update(cx, |terminal, cx| terminal.activate(1, window, cx));
    });
    debug.draw();
    let rerun = tr!("workbench-debug-debuggee-rerun-session").to_string();
    let named = debug.cx.update(|window, cx| {
        (fe_ui::nodes(window, cx).iter()).any(|node| node.label.as_ref() == rerun)
    });
    assert!(named, "the button is named {rerun}");
    let button = debug
        .cx
        .debug_bounds("terminal-debuggee-rerun")
        .expect("the debuggee tab's rerun button");
    debug
        .cx
        .simulate_click(button.center(), gpui::Modifiers::none());
    debug.draw();
    let again = debug.fake(1);
    debug.until("the new runInTerminal", |_| {
        (again.sent().iter()).any(|message| matches!(message, Message::Request(request) if request.command == "runInTerminal"))
    });
    debug.until("the new argv", |debug| {
        debug.started().last().cloned().flatten() == Some((next.program.path.clone(), Vec::new()))
    });
    let runs = debug.started();
    let old = runs
        .iter()
        .filter(|run| **run == Some((program.clone(), Vec::new())));
    assert_eq!(old.count(), 1, "the old argv never reran: {runs:?}");
    assert_eq!(debug.tabs().len(), 2, "the configuration's tab is reused");
}

/// C6 (§8): `debug.log_payloads` in the user's settings applies to the
/// next session, not the running one.
#[gpui::test]
fn changing_log_payloads_applies_to_the_next_session(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| spinning(script(root)));
    debug.start("main");
    debug.fake(0);
    debug.wait_state("running", |state| *state == SessionState::Running);
    let program = debug.root.join("target/debug/main");
    let program = program.to_string_lossy().into_owned();
    assert!(!debug.log().contains(&program), "metadata only");

    debug.set_log_payloads(true);
    let on = debug.debugger();
    assert!(on.read_with(debug.cx, |debugger, _| debugger.settings().log_payloads));
    assert!(
        !debug.log().contains(&program),
        "the running session's is as it was"
    );

    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
    debug.start("main");
    debug.fake(1);
    debug.wait_state("running again", |state| *state == SessionState::Running);
    assert!(debug.log().contains(&program), "{}", debug.log());
}

/// C6 (§5 "Sending"): a file's breakpoints go again when the set changes
/// and when a save moves them, but not as unsaved edits move them.
#[gpui::test]
fn breakpoints_are_sent_on_change_and_on_save_not_per_keystroke(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| spinning(script(root)));
    let main = main_path(&debug.root);
    let pane = debug
        .workbench
        .read_with(debug.cx, |workbench, _| workbench.pane().clone());
    let opened = main.clone();
    debug
        .cx
        .update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    debug.draw();
    let editor = debug.active_editor().expect("main.rs shows");
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("running", |state| *state == SessionState::Running);
    assert_eq!(sent_lines(&fake, &main), Vec::<Vec<i64>>::new());

    debug.toggle(&main, 1);
    debug.until("the added breakpoint", |_| {
        sent_lines(&fake, &main).len() == 1
    });
    assert_eq!(sent_lines(&fake, &main), [vec![2]]);

    // Edits above it move it in the buffer, but the program has the text
    // on disk.
    debug.cx.update(|window, cx| {
        window.focus(&editor.focus_handle(cx), cx);
        editor.update(cx, |editor, cx| editor.select_and_reveal(0..0, cx));
    });
    debug.cx.simulate_input("// one\n");
    debug.cx.simulate_input("// two\n");
    thread::sleep(Duration::from_millis(100));
    debug.draw();
    let rows = debug.store().read_with(debug.cx, |store, _| {
        (store.breakpoints(&main).iter())
            .map(|breakpoint| breakpoint.line)
            .collect::<Vec<_>>()
    });
    assert_eq!(rows, [4], "moved in the buffer");
    assert_eq!(
        sent_lines(&fake, &main),
        [vec![2]],
        "not sent per keystroke"
    );

    debug.cx.update(|window, cx| {
        editor
            .update(cx, |editor, cx| editor.save(window, cx))
            .detach();
    });
    debug.until("the saved lines", |_| sent_lines(&fake, &main).len() == 2);
    assert_eq!(sent_lines(&fake, &main), [vec![2], vec![4]]);
}

/// C7 (§10): a launch Linux refuses to trace names
/// `kernel.yama.ptrace_scope` and its value, read through the host's seam.
#[gpui::test]
fn a_ptrace_refusal_names_ptrace_scope(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        faults: vec![Fault::new(
            "launch",
            FaultKind::Fail("ptrace: Operation not permitted".to_owned()),
        )],
        ..script(root)
    });
    debug.start("main");
    debug.fake(0);
    debug.wait_state("the failure", SessionState::is_ended);
    let message = tr!("workbench-debug-ptrace-refused", scope = "2").to_string();
    assert!(
        message.contains("kernel.yama.ptrace_scope is 2"),
        "{message}"
    );
    debug.until("the toast", |debug| {
        (debug.toasts().iter()).any(|(level, said, _)| *level == Level::Error && *said == message)
    });
    assert_eq!(debug.scope_reads.load(Ordering::SeqCst), 1);
}

/// What a refusal is, by the adapter's message and the OS.
#[test]
fn refusals_are_told_by_their_messages() {
    use crate::debug_session::launch::{Refusal, refusal};
    let native = AdapterSpec::new("lldb-dap", "lldb-dap");
    let python = AdapterSpec::new("debugpy", "python3");
    let scope = || Some("1".to_owned());
    assert_eq!(
        refusal("error: ptrace failed", &python, HostOs::Linux, scope),
        Some(Refusal::Ptrace(Some("1".to_owned())))
    );
    assert_eq!(
        refusal("Operation not permitted", &native, HostOs::Linux, || None),
        Some(Refusal::Ptrace(None))
    );
    assert_eq!(
        refusal("Operation not permitted", &python, HostOs::Linux, scope),
        None
    );
    assert_eq!(
        refusal("missing get-task-allow", &native, HostOs::MacOs, scope),
        Some(Refusal::Signing)
    );
    assert_eq!(refusal("ptrace", &native, HostOs::MacOs, scope), None);
}

/// The launch arguments: the adapter's defaults, the typed fields under
/// the adapter's own keys, `adapter_args` last; and a `cwd` only a start
/// can expand is checked then, as a launch file's is when it's read.
#[cfg(unix)]
#[test]
fn launch_arguments_are_built_and_an_expanded_cwd_stays_in_the_root() {
    use crate::debug_session::launch::{Resolve, session_options};
    use crate::launch_defs::{
        AdapterRef, Console, Cwd, LaunchConfig, Program, Request, Source as ConfigSource,
        variables::{ActiveFile, Values},
    };
    let root = PathBuf::from("/project");
    let mut spec = AdapterSpec::new("debugpy", "/usr/bin/python3");
    spec.transport = AdapterTransport::Port(1);
    spec.launch_defaults
        .insert("justMyCode".into(), Value::Bool(true));
    let config = |cwd: Cwd| LaunchConfig {
        name: "tool".into(),
        source: ConfigSource::File,
        adapter: AdapterRef::Named {
            written: "debugpy".into(),
            name: Some("debugpy".into()),
        },
        request: Request::Launch,
        program: Some(Program::Written("${file}".into())),
        args: vec!["--token".into(), "${env:TOKEN}".into()],
        cwd,
        env: [("MODE".to_owned(), "fast".to_owned())].into(),
        stop_on_entry: true,
        pre_launch: None,
        console: Some(Console::Integrated),
        adapter_args: [("justMyCode".to_owned(), Value::Bool(false))]
            .into_iter()
            .collect(),
        blockers: Vec::new(),
    };
    let resolve = |cwd: Cwd| Resolve {
        config: config(cwd),
        root: root.clone(),
        values: Values {
            workspace_folder: root.clone(),
            active: Some(ActiveFile {
                path: root.join("pkg/tool.py"),
                line: 3,
            }),
            user_home: None,
            env: [("TOKEN".to_owned(), "s3cret".to_owned())].into(),
        },
        program: None,
        specs: vec![spec.clone()],
        login: None,
        settings: Default::default(),
        breakpoints: Default::default(),
    };
    let options = session_options(resolve(Cwd::Expand("${fileDirname}".into()))).unwrap();
    let arguments = Value::Object(options.arguments.clone());
    assert_eq!(arguments["program"], "/project/pkg/tool.py");
    assert_eq!(arguments["args"], serde_json::json!(["--token", "s3cret"]));
    assert_eq!(arguments["cwd"], "/project/pkg");
    assert_eq!(arguments["env"], serde_json::json!({"MODE": "fast"}));
    assert_eq!(arguments["stopOnEntry"], true);
    assert_eq!(arguments["console"], "integratedTerminal");
    assert_eq!(arguments["justMyCode"], false, "adapter_args last");
    assert_eq!(options.cwd, root.join("pkg"));
    assert_eq!(
        options.expansions,
        [fe_dap::EnvExpansion::new("TOKEN", "s3cret")]
    );

    let outside = session_options(resolve(Cwd::Expand("${fileDirname}/../..".into())));
    assert_eq!(
        outside.unwrap_err(),
        tr!(
            "workbench-debug-launch-cwd-outside",
            cwd = "${fileDirname}/../.."
        )
        .to_string()
    );
}

/// The debugger's events, as a subscriber hears them.
fn events(debug: &mut Debug) -> (Rc<RefCell<Vec<DebuggerEvent>>>, gpui::Subscription) {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let debugger = debug.debugger();
    let subscription = debug.cx.update(|_, cx| {
        let heard = heard.clone();
        cx.subscribe(&debugger, move |_, event: &DebuggerEvent, _| {
            heard.borrow_mut().push(event.clone())
        })
    });
    (heard, subscription)
}

/// The responses the fake got to its `runInTerminal`, by success.
fn terminal_replies(fake: &FakeAdapter) -> Vec<bool> {
    (fake.received().into_iter())
        .filter_map(|message| match message {
            Message::Response(response) if response.command == "runInTerminal" => {
                Some(response.success)
            }
            _ => None,
        })
        .collect()
}

/// R1 F1: a `runInTerminal` that comes after the session was stopped
/// runs nothing, and a debuggee tab whose program hasn't started when its
/// session stops closes, so it can't start later.
#[gpui::test]
fn a_stopped_sessions_terminal_requests_run_nothing(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        run_in_terminal: true,
        faults: vec![Fault::new(
            "initialize",
            FaultKind::ReplyAfter(Trigger::Manual),
        )],
        ..script(root)
    });
    debug.start("main");
    let fake = debug.fake(0);
    fake.wait_until("initialize", |received, _| {
        received
            .iter()
            .any(|m| matches!(m, Message::Request(r) if r.command == "initialize"))
    });
    // The request comes while nothing is drawn, and the session is stopped
    // before it's handled.
    fake.release();
    fake.wait_until("runInTerminal", |_, sent| {
        sent.iter()
            .any(|m| matches!(m, Message::Request(r) if r.command == "runInTerminal"))
    });
    let workbench = debug.workbench.clone();
    workbench.update(debug.cx, |workbench, cx| workbench.debug_stop(cx));
    debug.wait_state("the end", SessionState::is_ended);
    thread::sleep(Duration::from_millis(50));
    // Refused, if the adapter is still there to hear it.
    assert!(!terminal_replies(&fake).contains(&true), "never started");
    assert_eq!(debug.tabs(), Vec::<String>::new(), "no debuggee tab");
    assert_eq!(debug.started(), Vec::new(), "nothing ran");

    // A tab whose program hasn't started closes when its session stops.
    let terminal = debug.terminal();
    terminal.update(debug.cx, |terminal, _| terminal.debuggees_unstarted = true);
    debug.launcher.set_script(Script {
        run_in_terminal: true,
        ..script(&debug.root)
    });
    debug.start("main");
    let fake = debug.fake(1);
    debug.until("the debuggee tab", |debug| debug.tabs() == ["main"]);
    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
    // The last tab gone, the dock hides: no frame draws the strip.
    debug.until("the tab to close", |debug| {
        let terminal = debug.terminal();
        terminal.read_with(debug.cx, |terminal, _| terminal.views().count()) == 0
    });
    assert!(
        !terminal_replies(&fake).contains(&true),
        "it was never said to have started"
    );
}

/// R1 F2: a window opened with `[debug]` settings already loaded starts
/// its sessions with them, with no reload after.
#[gpui::test]
fn a_new_window_takes_the_debug_settings_already_loaded(cx: &mut TestAppContext) {
    let setup = Setup {
        user_settings: Some("[debug]\nlog_payloads = true\n"),
        ..Setup::default()
    };
    let mut debug = untrusted_with(cx, &launch_file(), |root| spinning(script(root)), setup);
    debug.set_trust(true);
    let debugger = debug.debugger();
    assert!(debugger.read_with(debug.cx, |debugger, _| debugger.settings().log_payloads));
    debug.start("main");
    debug.fake(0);
    debug.wait_state("running", |state| *state == SessionState::Running);
    let program = debug.root.join("target/debug/main");
    assert!(debug.log().contains(program.to_string_lossy().as_ref()));
}

/// R1 F3: a rerun is a start from the moment it's asked for: Stop gives
/// it up while its configurations are read, and a later start replaces it.
#[gpui::test]
fn stop_gives_up_a_rerun_still_reading_and_the_newest_start_wins(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), script);
    let workbench = debug.workbench.clone();
    debug.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.rerun_debug_named("main".to_owned(), window, cx);
            workbench.debug_stop(cx);
        });
    });
    debug.draw();
    thread::sleep(Duration::from_millis(50));
    debug.draw();
    assert_eq!(debug.launcher.launched().len(), 0, "stopped while reading");
    assert_eq!(debug.state(), None);

    debug.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.rerun_debug_named("main".to_owned(), window, cx);
            workbench.rerun_debug_named("built".to_owned(), window, cx);
        });
    });
    debug.draw();
    assert_eq!(debug.tabs(), ["build"], "the newer start's pre-launch task");
    thread::sleep(Duration::from_millis(50));
    debug.draw();
    assert_eq!(
        debug.launcher.launched().len(),
        0,
        "the older start never launches"
    );
    let debugger = debug.debugger();
    let starting = debugger.read_with(debug.cx, |debugger, _| {
        debugger.starting().map(str::to_owned)
    });
    assert_eq!(starting.as_deref(), Some("built"));
}

/// R1 F4: a run read at exit keeps its last lines whatever the terminal's
/// scrollback, so a long message wrapped over many rows is read whole, from
/// a real terminal.
#[gpui::test]
fn a_read_run_keeps_its_output_whatever_the_scrollback(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &[], script);
    let terminal = debug.terminal();
    terminal.update(debug.cx, |terminal, cx| {
        terminal.fake_sessions = false;
        let settings = fe_config::TerminalSettings {
            scrollback: 0,
            ..fe_config::TerminalSettings::default()
        };
        terminal.set_settings(&settings, cx);
    });
    let built = debug.root.join("target").join("debug").join("tool");
    let artifact = serde_json::json!({
        "reason": "compiler-artifact",
        "target": {"name": "tool", "kind": ["bin"], "src_path": "x".repeat(3000)},
        "executable": built.to_string_lossy(),
    })
    .to_string();
    // The artifact, then far more output than the grid keeps.
    let script = "i=0; while [ $i -lt 300 ]; do echo line $i; i=$((i+1)); done; \
        printf '%s\\n' \"$1\"; \
        i=0; while [ $i -lt 20000 ]; do echo after $i; i=$((i+1)); done";
    let task = Task {
        label: "build".to_owned(),
        command: TaskCommand::Argv(
            ["/bin/sh", "-c", script, "sh", &artifact]
                .map(str::to_owned)
                .to_vec(),
        ),
        cwd: debug.root.clone(),
        env: Default::default(),
        group: None,
        default: false,
        source: Source::File,
        problem_matcher: None,
    };
    let heard: Rc<RefCell<Option<Option<TaskExit>>>> = Rc::default();
    let workbench = debug.workbench.clone();
    debug.cx.update(|window, cx| {
        let heard = heard.clone();
        workbench.update(cx, |workbench, cx| {
            let reading: ExitHeard = Box::new(move |_, exit, _| *heard.borrow_mut() = Some(exit));
            let keep = launch::is_artifact_message;
            assert!(workbench.run_task_reading(task, keep, reading, window, cx));
        });
    });
    debug.until("the run's exit", |_| heard.borrow().is_some());
    let exit = heard.take().flatten().expect("it exited");
    assert_eq!(exit.code, Some(0));
    assert_eq!(
        exit.lines,
        std::slice::from_ref(&artifact),
        "only the artifact line kept"
    );
    let joined = exit.lines.join("\n");
    assert_eq!(launch::cargo_artifact(&joined, "tool"), Some(built));
    let grid = terminal.read_with(debug.cx, |terminal, cx| {
        let view = terminal.view().expect("the run's tab");
        view.read(cx)
            .terminal()
            .map(|terminal| terminal.output_text())
    });
    assert!(
        !grid.expect("a real session").contains("line 0\n"),
        "the grid dropped the start"
    );
}

/// R1 F5: breakpoints changed while the configuration is resolved go with
/// the session.
#[gpui::test]
fn breakpoints_changed_while_resolving_go_with_the_session(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| spinning(script(root)));
    let main = main_path(&debug.root);
    debug.start("built");
    let terminal = debug.terminal();
    let view = terminal
        .read_with(debug.cx, |terminal, _| terminal.view().cloned())
        .expect("the build's tab");
    let store = debug.store();
    let path = main.clone();
    // Its build passes, and its configuration is resolved off the main
    // thread: a breakpoint added before that lands goes with the session.
    debug.cx.update(|_, cx| {
        view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(0))));
        store.update(cx, |store, cx| store.toggle(&path, 1, cx));
    });
    let fake = debug.fake(0);
    debug.wait_state("the stop at it", |state| *state == SessionState::Stopped);
    assert_eq!(sent_lines(&fake, &main), [vec![2]]);
}

/// R1 F6: a late reply to an earlier send of the same lines doesn't
/// replace the last send's.
#[gpui::test]
fn only_the_last_sends_reply_counts(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        faults: vec![Fault::new("setBreakpoints", FaultKind::ReplyAfter(Trigger::Manual)).nth(1)],
        ..spinning(script(root))
    });
    let main = main_path(&debug.root);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("running", |state| *state == SessionState::Running);
    debug.toggle(&main, 1);
    debug.until("the held send", |_| sent_lines(&fake, &main).len() == 1);
    debug.toggle(&main, 3);
    debug.toggle(&main, 3);
    debug.until("both later sends", |_| sent_lines(&fake, &main).len() == 3);
    let debugger = debug.debugger();
    let ids = |debug: &mut Debug| {
        debugger.read_with(debug.cx, |debugger, _| {
            (debugger.verification(&main).iter())
                .map(|verification| (verification.line, verification.id))
                .collect::<Vec<_>>()
        })
    };
    debug.until("the last send's reply", |debug| {
        ids(debug) == [(2, Some(4))]
    });
    fake.release();
    debug.until("the held reply", |_| {
        (fake.sent().iter())
            .filter(|m| matches!(m, Message::Response(r) if r.command == "setBreakpoints"))
            .count()
            == 3
    });
    debug.draw();
    assert_eq!(ids(&mut debug), [(2, Some(4))], "the late reply is dropped");
}

/// R1 F7: a variable the adapter removes from the program's environment
/// is removed, by `env -u`, the program still run directly.
#[cfg(unix)]
#[gpui::test]
fn a_debuggee_env_removal_is_removed(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &[], script);
    let terminal = debug.terminal();
    let debuggee = Debuggee {
        name: "main".to_owned(),
        argv: ["/bin/sh", "-c", "echo ${HOME-unset} $KEPT"]
            .map(str::to_owned)
            .to_vec(),
        cwd: debug.root.clone(),
        env: [
            ("HOME".to_owned(), None),
            ("KEPT".to_owned(), Some("yes".to_owned())),
        ]
        .into(),
    };
    debug.cx.update(|window, cx| {
        terminal.update(cx, |terminal, cx| {
            terminal.add_debuggee(&debuggee, window, cx).unwrap();
        });
    });
    let options = terminal.read_with(debug.cx, |terminal, _| {
        terminal.started_options.last().cloned().expect("a session")
    });
    let (program, args) = options.shell.expect("a program");
    assert_eq!(program, "/usr/bin/env");
    assert_eq!(args[..3], ["-u", "HOME", "--"]);
    assert_eq!(options.env, [("KEPT".to_owned(), "yes".to_owned())].into());
    let ran = std::process::Command::new(&program)
        .args(&args)
        .env("KEPT", "yes")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "unset yes\n");

    // A program whose path holds an `=`, which `env` would read as a
    // variable to set, is still run with its arguments as they are.
    let odd = debug.root.join("a=b").join("prog");
    stand_in(&odd, "echo ${HOME-unset} \"$@\"");
    let debuggee = Debuggee {
        argv: vec![
            odd.to_string_lossy().into_owned(),
            "x  y".to_owned(),
            "$z".to_owned(),
        ],
        ..debuggee
    };
    debug.cx.update(|window, cx| {
        terminal.update(cx, |terminal, cx| {
            terminal.add_debuggee(&debuggee, window, cx).unwrap();
        });
    });
    let options = terminal.read_with(debug.cx, |terminal, _| {
        terminal.started_options.last().cloned().expect("a session")
    });
    let (program, args) = options.shell.expect("a program");
    let ran = std::process::Command::new(&program)
        .args(&args)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "unset x  y $z\n");
}

/// R1 F8 and F12: a start under way is told and can be stopped, and one
/// whose pre-launch task's tab closes is given up.
#[gpui::test]
fn a_start_under_way_is_told_stoppable_and_given_up_with_its_task(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), script);
    let (heard, _subscription) = events(&mut debug);
    debug.start("built");
    let debugger = debug.debugger();
    let controls = debugger.read_with(debug.cx, |debugger, _| debugger.controls());
    assert!(controls.stop.is_enabled(), "Stop gives the start up");
    assert!(!controls.continue_.is_enabled());
    assert_eq!(
        *heard.borrow(),
        [DebuggerEvent::Starting("built".to_owned())]
    );

    // Its task's tab closes before the task exits.
    debug.run("terminal.close");
    debug.draw();
    assert_eq!(
        heard.borrow().last(),
        Some(&DebuggerEvent::StartGivenUp),
        "{:?}",
        heard.borrow()
    );
    let (starting, controls) = debugger.read_with(debug.cx, |debugger, _| {
        (debugger.starting().map(str::to_owned), debugger.controls())
    });
    assert_eq!(starting, None);
    assert!(!controls.stop.is_enabled());

    // And `debug.stop` gives one up.
    debug.start("built");
    debug.run("debug.stop");
    assert_eq!(heard.borrow().last(), Some(&DebuggerEvent::StartGivenUp));
    debug.exit_active(Some(0));
    thread::sleep(Duration::from_millis(50));
    debug.draw();
    assert_eq!(debug.launcher.launched().len(), 0);
}

/// R1 F10: a Continue or a breakpoint update the adapter refuses is said
/// in a toast.
#[gpui::test]
fn a_refused_continue_or_breakpoint_update_is_said(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        faults: vec![
            Fault::new("continue", FaultKind::Fail("busy".to_owned())),
            Fault::new("setBreakpoints", FaultKind::Fail("no such file".to_owned())).nth(2),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    let failed = tr!("workbench-debug-request-failed", name = "main").to_string();
    let said = |debug: &mut Debug| {
        (debug.toasts().iter())
            .filter(|(level, message, _)| *level == Level::Warning && *message == failed)
            .count()
    };
    debug.run("debug.start");
    debug.until("the refused continue", |debug| said(debug) == 1);
    assert!(fake.commands().contains(&"continue".to_owned()));

    debug.toggle(&main, 0);
    debug.until("the refused update", |debug| said(debug) == 2);
}

/// R1 F11: `debug.timeouts` applies to the next session: a launch given
/// 2 s ends after 2 s.
#[gpui::test]
fn the_debug_timeouts_apply_to_the_next_session(cx: &mut TestAppContext) {
    let clock = Arc::new(ManualClock::new());
    let setup = Setup {
        clock: Some(clock.clone()),
        ..Setup::default()
    };
    let unanswered = |root: &Path| Script {
        startup: Startup::LaunchResponseFirst,
        faults: vec![Fault::new("launch", FaultKind::Unanswered)],
        ..script(root)
    };
    let mut debug = untrusted_with(cx, &launch_file(), unanswered, setup);
    debug.set_trust(true);
    let mut settings = fe_config::Settings::default();
    settings.debug.timeouts.launch = 2_000;
    debug.reload(settings);
    debug.start("main");
    let fake = debug.fake(0);
    debug.until("the launch", |_| !requests(&fake, "launch").is_empty());
    clock.wait_for_sleep(Duration::from_secs(2));
    clock.advance(Duration::from_secs(2));
    debug.wait_state("the timeout", SessionState::is_ended);
    let Some(SessionState::Ended(ended)) = debug.state() else {
        unreachable!()
    };
    assert_eq!(
        ended.reason,
        EndReason::Failed(SessionError::StartTimeout(Duration::from_secs(2)))
    );
}

/// Clicks the active debuggee tab's rerun button, the debuggee tab being
/// the `ix`th.
fn click_rerun(debug: &mut Debug, ix: usize) {
    let terminal = debug.terminal();
    debug.cx.update(|window, cx| {
        terminal.update(cx, |terminal, cx| terminal.activate(ix, window, cx));
    });
    debug.draw();
    // In the panel's row, or with one session the dock strip's.
    let button = debug
        .cx
        .debug_bounds("icon-button:terminal-debuggee-rerun-button")
        .expect("the debuggee tab's rerun button");
    debug
        .cx
        .simulate_click(button.center(), gpui::Modifiers::none());
    debug.draw();
}

/// R2 F13: a debuggee tab's rerun works in a workspace opened through a
/// symlink.
#[cfg(unix)]
#[gpui::test]
fn a_debuggee_tab_reruns_in_a_symlinked_workspace(cx: &mut TestAppContext) {
    let setup = Setup {
        symlinked: true,
        ..Setup::default()
    };
    let in_terminal = |root: &Path| Script {
        run_in_terminal: true,
        ..script(root)
    };
    let mut debug = untrusted_with(cx, &launch_file(), in_terminal, setup);
    let opened = debug.workbench.read_with(debug.cx, |workbench, _| {
        workbench.root().map(Path::to_owned)
    });
    assert_ne!(
        opened.as_deref(),
        Some(debug.root.as_path()),
        "opened by its link"
    );
    debug.set_trust(true);
    debug.start("main");
    debug.fake(0);
    debug.wait_state("the end", SessionState::is_ended);
    debug.exit_active(Some(0));
    click_rerun(&mut debug, 0);
    debug.fake(1);
}

/// R2 F14: an adapter's project resources are the workspace's: debugpy is
/// the root's `.venv` Python, though the program runs in a subfolder.
#[cfg(unix)]
#[test]
fn a_root_venv_is_found_for_a_nested_cwd() {
    use crate::debug_session::launch::{Resolve, session_options};
    use crate::launch_defs::{AdapterRef, Cwd, Program};
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let python = root.join(".venv/bin/python");
    stand_in(&python, "exit 0");
    fs::create_dir_all(root.join("tools")).unwrap();
    let config = LaunchConfig {
        adapter: AdapterRef::Named {
            written: "debugpy".into(),
            name: Some("debugpy".into()),
        },
        program: Some(Program::Written("main.py".into())),
        cwd: Cwd::At(root.join("tools")),
        ..config_named("tool")
    };
    let options = session_options(Resolve {
        breakpoints: Default::default(),
        ..resolve(config, &root, None)
    })
    .unwrap();
    assert_eq!(options.adapter.command, python);
    assert_eq!(options.cwd, root.join("tools"));
}

/// R2 F15: a launch file's lldb-dap configuration in a Cargo project gets
/// the toolchain's Rust formatters, as a detected one does.
#[cfg(unix)]
#[test]
fn a_named_lldb_dap_rust_configuration_gets_the_rust_formatters() {
    use crate::debug_session::launch::session_options;
    use crate::launch_defs::{AdapterRef, Program};
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"tool\"\n").unwrap();
    let bin = root.join("bin");
    stand_in(&bin.join("lldb-dap"), "exit 0");
    stand_in(&bin.join("rustc"), "echo /fake/sysroot");
    let shell = root.join("login-shell");
    stand_in(
        &shell,
        &format!("PATH='{}' /bin/sh -c \"$3\"", bin.display()),
    );
    let login = (0..5)
        .map(|_| Arc::new(fe_lsp::LoginPath::new(&shell)))
        .find(|login| login.path().is_some())
        .expect("the stand-in login shell's PATH");
    let config = LaunchConfig {
        adapter: AdapterRef::Named {
            written: "lldb-dap".into(),
            name: Some("lldb-dap".into()),
        },
        program: Some(Program::Written(
            "${workspaceFolder}/target/debug/tool".into(),
        )),
        ..config_named("tool")
    };
    let options = session_options(resolve(config, &root, Some(login))).unwrap();
    assert_eq!(options.adapter.command, bin.join("lldb-dap"));
    let lookup = "/fake/sysroot/lib/rustlib/etc/lldb_lookup.py";
    let commands = Value::Object(options.arguments.clone())["initCommands"].to_string();
    assert!(commands.contains(lookup), "{commands}");
}

/// A stand-in program at `path` running `body`.
#[cfg(unix)]
fn stand_in(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A launch file configuration `name` in the root, to start from.
fn config_named(name: &str) -> LaunchConfig {
    use crate::launch_defs::{AdapterRef, Cwd, Request, Source as ConfigSource};
    LaunchConfig {
        name: name.into(),
        source: ConfigSource::File,
        adapter: AdapterRef::Named {
            written: "fake".into(),
            name: Some("fake".into()),
        },
        request: Request::Launch,
        program: None,
        args: Vec::new(),
        cwd: Cwd::At(PathBuf::new()),
        env: Default::default(),
        stop_on_entry: false,
        pre_launch: None,
        console: None,
        adapter_args: Default::default(),
        blockers: Vec::new(),
    }
}

/// What `config` of the project at `root` resolves with: no active editor,
/// no settings, bare commands on `login`.
fn resolve(
    config: LaunchConfig,
    root: &Path,
    login: Option<Arc<fe_lsp::LoginPath>>,
) -> crate::debug_session::launch::Resolve {
    use crate::launch_defs::{Cwd, variables::Values};
    let config = match config.cwd {
        Cwd::At(ref cwd) if cwd.as_os_str().is_empty() => LaunchConfig {
            cwd: Cwd::At(root.to_owned()),
            ..config
        },
        _ => config,
    };
    crate::debug_session::launch::Resolve {
        config,
        root: root.to_owned(),
        values: Values {
            workspace_folder: root.to_owned(),
            ..Values::default()
        },
        program: None,
        specs: Vec::new(),
        login,
        settings: Default::default(),
        breakpoints: Default::default(),
    }
}

/// R2 F16: a second stop, whose stack never comes, leaves no place from
/// the first, and is told.
#[gpui::test]
fn a_new_stop_retires_the_last_stops_place(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        faults: vec![Fault::new("stackTrace", FaultKind::Unanswered).nth(2)],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.toggle(&main, 3);
    let (heard, _subscription) = events(&mut debug);
    debug.start("main");
    let fake = debug.fake(0);
    let debugger = debug.debugger();
    let place = |debug: &mut Debug| {
        debugger.read_with(debug.cx, |debugger, _| {
            debugger.stop_location().map(|stop| stop.row)
        })
    };
    debug.until("the first stop's place", |debug| place(debug) == Some(1));
    let stopped = |heard: &[DebuggerEvent]| {
        (heard.iter())
            .filter(|event| **event == DebuggerEvent::State(SessionState::Stopped))
            .count()
    };
    let first = stopped(&heard.borrow());
    debug.run("debug.start");
    debug.until("the second stop's fetch", |_| {
        requests(&fake, "stackTrace").len() == 2
    });
    debug.until("the second stop told", |_| stopped(&heard.borrow()) > first);
    assert_eq!(debug.state(), Some(SessionState::Stopped));
    assert_eq!(place(&mut debug), None, "the first stop's place is over");
}

/// A `thread` event the fake sends as its own.
fn thread_event(reason: &str, thread_id: i64) -> Message {
    use fe_dap::protocol::{EventMessage, ThreadBody, event};
    let body = ThreadBody {
        reason: reason.into(),
        thread_id,
        ..ThreadBody::default()
    };
    Message::Event(EventMessage::new::<event::ThreadEvent>(0, &body))
}

/// R3 F16: the shown thread exiting moves the stop to another thread at a
/// new epoch, still Stopped (fe-dap's retained stop): the last place is
/// over at once and told, though the new stack never comes.
#[gpui::test]
fn a_retained_stop_retires_the_last_place_and_is_told(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        faults: vec![Fault::new("stackTrace", FaultKind::Unanswered).nth(2)],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    let fake = debug.fake(0);
    // A second thread, then the stop on both.
    fake.send(thread_event("started", 2));
    let debugger = debug.debugger();
    let place = |debug: &mut Debug| {
        debugger.read_with(debug.cx, |debugger, _| {
            debugger.stop_location().map(|stop| (stop.row, stop.epoch))
        })
    };
    debug.until("the stop's place", |debug| place(debug).is_some());
    let (heard, _subscription) = events(&mut debug);
    // The shown thread exits: the stop moves to thread 2, whose stack the
    // fake never answers.
    fake.send(thread_event("exited", fe_dap::fake::THREAD_ID));
    debug.until("the retained stop's fetch", |_| {
        requests(&fake, "stackTrace").len() == 2
    });
    debug.until("the change told", |_| {
        heard
            .borrow()
            .contains(&DebuggerEvent::State(SessionState::Stopped))
    });
    assert_eq!(debug.state(), Some(SessionState::Stopped), "still stopped");
    assert_eq!(place(&mut debug), None, "the last place is over");
}

/// R3 F17: a stopped file still being read when its stop ends never opens,
/// nor gets the old stop's caret.
#[gpui::test]
fn a_stopped_file_read_after_its_stop_ends_never_opens(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), script);
    let (gate, held) = async_channel::unbounded();
    debug
        .cx
        .update(|_, cx| cx.set_global(crate::debug_session::StopOpenGate(held)));
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    let debugger = debug.debugger();
    debug.until("the stop's place", |debug| {
        debugger.read_with(debug.cx, |debugger, _| debugger.stop_location().is_some())
    });
    assert_eq!(debug.caret(), None, "main.rs is still being read");
    debug.run("debug.start");
    debug.until("the continue", |_| {
        fake.commands().contains(&"continue".to_owned())
    });
    debug.wait_state("past the stop", |state| *state != SessionState::Stopped);
    gate.try_send(()).ok();
    debug.draw();
    thread::sleep(Duration::from_millis(50));
    debug.draw();
    let open = debug.workbench.read_with(debug.cx, |workbench, cx| {
        workbench.pane().read(cx).position(&main)
    });
    assert_eq!(open, None, "the old stop's file never opened");
}

/// R3 F18: a debuggee tab whose process started just before its session
/// stops (before the start is noticed) keeps its tab and output.
#[gpui::test]
fn a_debuggee_started_just_before_the_stop_keeps_its_tab(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, &launch_file(), |root| Script {
        run_in_terminal: true,
        ..script(root)
    });
    let terminal = debug.terminal();
    terminal.update(debug.cx, |terminal, _| terminal.debuggees_unstarted = true);
    debug.start("main");
    debug.fake(0);
    debug.until("the debuggee tab", |debug| debug.tabs() == ["main"]);
    // Its process starts, and the session stops in the same turn, before
    // any poll sees the start.
    let workbench = debug.workbench.clone();
    debug.cx.update(|_, cx| {
        terminal.update(cx, |terminal, _| terminal.debuggees_unstarted = false);
        workbench.update(cx, |workbench, cx| workbench.debug_stop(cx));
    });
    debug.wait_state("the end", SessionState::is_ended);
    thread::sleep(Duration::from_millis(60));
    debug.draw();
    let views = terminal.read_with(debug.cx, |terminal, _| terminal.views().count());
    assert_eq!(views, 1, "the started debuggee keeps its tab");
}
