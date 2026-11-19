//! Card D8's commands (`debug_session/commands.rs`) in a headless
//! workbench against fe-dap's fake adapter, one test per command: the
//! steps, Continue, Pause, Restart, run to cursor's one-shot breakpoint,
//! start with arguments, the session picker and the breakpoint commands.
//! The F-keys go through the workbench root's debug flags here too. The
//! harness ([`Debug`]) is the debug toolbar's tests' as well.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_dap::{
    AdapterSpec, AdapterTransport, HostOs, Message, SessionState, SystemClock,
    fake::{FakeAdapter, FakeLauncher, Fault, FaultKind, Line, Script, Trigger},
};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;
use serde_json::Value;

use crate::{
    Launch, Workbench,
    debug_breakpoints::BreakpointStore,
    debug_session::{
        DebugHost, Debugger,
        commands::{ARGS_PICKER, Incomplete, SESSION_PICKER, split_args},
        set_debug_host,
    },
    task_commands::TestFiles,
    task_defs::ProjectFiles,
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

/// The program the fake debugs: `src/main.rs`'s five lines, the fourth in
/// `add`.
const MAIN: &str = "let x = 1;\nlet point = 2;\nlet y = add(x, 2);\na + b\nprintln!(y);\n";

/// The launch file: `main` runs at once, and so does `other`.
const LAUNCH: &str = r#"
[[launch]]
name = "main"
adapter = "fake"
program = "${workspaceFolder}/target/debug/main"

[[launch]]
name = "other"
adapter = "fake"
program = "${workspaceFolder}/target/debug/other"
"#;

/// A workbench on a project the fake debugs, its root trusted, with the
/// fake as the host's adapter `fake`.
pub(super) struct Debug<'a> {
    _dir: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) workbench: Entity<Workbench>,
    launcher: Arc<FakeLauncher>,
    pub(super) cx: &'a mut VisualTestContext,
}

/// The fake's script for the project at `root`: its program is
/// `src/main.rs`.
pub(super) fn script(root: &Path) -> Script {
    let mut script = Script::default();
    script.program.path = main_path(root).to_string_lossy().into_owned();
    script
}

pub(super) fn main_path(root: &Path) -> PathBuf {
    root.join("src").join("main.rs")
}

/// `script`, its program spinning on its last line until paused.
pub(super) fn spinning(mut script: Script) -> Script {
    let last = script.program.lines.len() - 1;
    script.program.lines[last] = Line {
        spin: true,
        ..script.program.lines[last].clone()
    };
    script
}

/// The workbench on a trusted project with `src/main.rs`, the fake
/// following `script` (made from the root).
pub(super) fn trusted<'a>(
    cx: &'a mut TestAppContext,
    script: impl FnOnce(&Path) -> Script,
) -> Debug<'a> {
    // The session's thread wakes the workbench's event loop.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(main_path(&root), MAIN).unwrap();
    fs::create_dir_all(root.join(".tenmoku")).unwrap();
    fs::write(root.join(".tenmoku/launch.toml"), LAUNCH).unwrap();
    // Its Rust file would start rust-analyzer, which would outlive the test.
    let settings = fe_config::project_settings_path(&root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
    let launcher = Arc::new(FakeLauncher::new(script(&root)));
    let host = DebugHost {
        launcher: Some(launcher.clone()),
        clock: Arc::new(SystemClock),
        specs: vec![AdapterSpec {
            types: vec!["fake".into()],
            languages: vec!["rust".into()],
            transport: AdapterTransport::Port(1),
            ..AdapterSpec::new("fake", "fake")
        }],
        ptrace_scope: Arc::new(|| None),
        login: None,
        os: HostOs::Linux,
    };
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_debug_host(cx, host);
        cx.set_global(TestFiles(Arc::new(Disk)));
    });
    let launch = Launch {
        root: Some(root.clone()),
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
        root,
        workbench,
        launcher,
        cx,
    };
    debug.draw();
    let workbench = debug.workbench.clone();
    workbench.update(debug.cx, |workbench, cx| workbench.set_root_trust(true, cx));
    debug.draw();
    debug
}

impl Debug<'_> {
    pub(super) fn draw(&mut self) {
        for _ in 0..2 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    /// Draws until `done` holds. Panics after 30 s, naming `what`.
    pub(super) fn until(&mut self, what: &str, done: impl Fn(&mut Self) -> bool) {
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

    /// The next session's requests time out after `ms`, as the user's
    /// `debug.timeouts.request` says.
    pub(super) fn set_request_timeout(&mut self, ms: u64) {
        let mut settings = fe_config::Settings::default();
        settings.debug.timeouts.request = ms;
        let event = fe_config::SettingsEvent {
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

    /// Runs workbench command `id` as a menu item would.
    pub(super) fn run(&mut self, id: &str) {
        let run = RunCommand(
            Invocation::of(id)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        self.draw();
    }

    /// Starts the configuration `name`, as `debug.rerun_last` does.
    pub(super) fn start(&mut self, name: &str) {
        let workbench = self.workbench.clone();
        let name = name.to_owned();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.rerun_debug_named(name, window, cx)
            });
        });
        self.draw();
    }

    pub(super) fn debugger(&mut self) -> Entity<Debugger> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.debugger(cx)))
            .expect("the window's debugger")
    }

    pub(super) fn state(&mut self) -> Option<SessionState> {
        let debugger = self.debugger();
        debugger.read_with(self.cx, |debugger, _| debugger.state())
    }

    pub(super) fn wait_state(&mut self, what: &str, done: impl Fn(&SessionState) -> bool) {
        self.until(what, |debug| debug.state().as_ref().is_some_and(&done));
    }

    /// The stopped frame's row, from 0, once the stop's frames came.
    pub(super) fn stopped_row(&mut self) -> Option<u32> {
        let debugger = self.debugger();
        debugger.read_with(self.cx, |debugger, _| {
            debugger.stop_location().map(|stop| stop.row)
        })
    }

    /// Waits for a stop at `row` (from 0), its frames come.
    pub(super) fn wait_stop_at(&mut self, row: u32) {
        self.until(&format!("the stop at row {row}"), |debug| {
            debug.state() == Some(SessionState::Stopped) && debug.stopped_row() == Some(row)
        });
    }

    /// The `n`th fake launched, from 0, once it has been.
    pub(super) fn fake(&mut self, n: usize) -> FakeAdapter {
        let launcher = self.launcher.clone();
        self.until(&format!("launch {n}"), |_| launcher.launched().len() > n);
        launcher.wait_for_launch(n)
    }

    pub(super) fn store(&mut self) -> Entity<BreakpointStore> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.breakpoint_store(cx)))
            .expect("rooted")
    }

    pub(super) fn toggle(&mut self, path: &Path, row: u32) {
        let store = self.store();
        let path = path.to_owned();
        store.update(self.cx, |store, cx| store.toggle(&path, row, cx));
        self.draw();
    }

    /// The store's breakpoints of the file at `path`: line and enabled.
    pub(super) fn breakpoints(&mut self, path: &Path) -> Vec<(u32, bool)> {
        let store = self.store();
        store.read_with(self.cx, |store, _| {
            (store.breakpoints(path).iter())
                .map(|breakpoint| (breakpoint.line, breakpoint.enabled))
                .collect()
        })
    }

    pub(super) fn active_editor(&mut self) -> Option<Entity<EditorView>> {
        self.workbench.read_with(self.cx, |workbench, cx| {
            workbench.active_editor(cx).cloned()
        })
    }

    /// Opens `path` in the active pane, focused, with the caret at the start
    /// of `row`.
    pub(super) fn open_at(&mut self, path: &Path, row: u32) -> Entity<EditorView> {
        let pane = self
            .workbench
            .read_with(self.cx, |workbench, _| workbench.pane().clone());
        let opened = path.to_owned();
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
        self.until("the file to open", |debug| {
            debug.active_editor().is_some_and(|editor| {
                editor.read_with(debug.cx, |editor, _| {
                    editor.model().path().is_some_and(|open| open == path)
                })
            })
        });
        let editor = self.active_editor().unwrap();
        self.caret_to(&editor, row);
        editor
    }

    /// Puts `editor`'s caret at the start of `row`, focused.
    pub(super) fn caret_to(&mut self, editor: &Entity<EditorView>, row: u32) {
        self.cx.update(|window, cx| {
            window.focus(&editor.focus_handle(cx), cx);
            editor.update(cx, |editor, cx| {
                let offset = editor.model().text().row_start(row);
                editor.select_and_reveal(offset..offset, cx);
            });
        });
        self.draw();
    }

    /// The active editor's file, and its caret's row.
    pub(super) fn caret(&mut self) -> Option<(PathBuf, u32)> {
        let editor = self.active_editor()?;
        editor.read_with(self.cx, |editor, _| {
            let model = editor.model();
            let row = (model.text())
                .offset_to_point(model.primary_selection().head())
                .row;
            Some((model.path()?.to_owned(), row))
        })
    }

    /// Each toast showing: level and message.
    pub(super) fn toasts(&mut self) -> Vec<(Level, String)> {
        let layer = self
            .workbench
            .read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| (entry.toast.level, entry.toast.message.clone()))
                .collect()
        })
    }

    /// The modal showing, if it's one of D8's pickers: its name and rows.
    fn command_picker(&mut self) -> Option<(&'static str, Vec<String>)> {
        let workbench = self.workbench.clone();
        let (id, rows) = self
            .cx
            .update(|_, cx| workbench.read(cx).debug_command_picker(cx))?;
        Some((id.0, rows.iter().map(ToString::to_string).collect()))
    }

    fn confirm_command_picker(&mut self) {
        let workbench = self.workbench.clone();
        self.cx
            .update(|window, cx| Workbench::confirm_debug_command_picker(&workbench, window, cx));
        self.draw();
    }
}

/// The arguments of each request `command` the fake got.
pub(super) fn requests(fake: &FakeAdapter, command: &str) -> Vec<Value> {
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
pub(super) fn sent_lines(fake: &FakeAdapter, path: &Path) -> Vec<Vec<i64>> {
    (requests(fake, "setBreakpoints").into_iter())
        .filter(|arguments| arguments["source"]["path"] == path.to_string_lossy().as_ref())
        .map(|arguments| {
            (arguments["breakpoints"].as_array().into_iter().flatten())
                .filter_map(|breakpoint| breakpoint["line"].as_i64())
                .collect()
        })
        .collect()
}

/// How many `command` requests the fake got.
fn count(fake: &FakeAdapter, command: &str) -> usize {
    (fake.commands().iter())
        .filter(|sent| *sent == command)
        .count()
}

/// A session stopped on the breakpoint on `row` of `src/main.rs`.
fn stopped_at<'a>(cx: &'a mut TestAppContext, row: u32) -> (Debug<'a>, FakeAdapter) {
    stopped_at_with(cx, row, script)
}

/// [`stopped_at`], the fake following `script`.
fn stopped_at_with<'a>(
    cx: &'a mut TestAppContext,
    row: u32,
    script: impl FnOnce(&Path) -> Script,
) -> (Debug<'a>, FakeAdapter) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, row);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(row);
    (debug, fake)
}

/// `debug.continue` runs on to the next breakpoint.
#[gpui::test]
fn continue_runs_to_the_next_breakpoint(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at(cx, 0);
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.until("the added breakpoint", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1, 3])
    });
    debug.run("debug.continue");
    debug.wait_stop_at(2);
    assert_eq!(count(&fake, "continue"), 1);
    // Nothing more to stop at: it runs to the end.
    debug.run("debug.continue");
    debug.wait_state("the end", SessionState::is_ended);
}

/// `debug.pause` stops a running program; with no session it says so.
#[gpui::test]
fn pause_stops_a_running_program(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| spinning(script(root)));
    debug.run("debug.pause");
    let none = tr!("workbench-debug-not-running").to_string();
    assert_eq!(debug.toasts(), [(Level::Info, none)]);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("running", |state| *state == SessionState::Running);
    // Pause is the running session's key, F6.
    debug.cx.simulate_keystrokes("f6");
    debug.until("the pause", |_| count(&fake, "pause") == 1);
    debug.wait_stop_at(4);
}

/// `debug.step_over` goes to the next line, by F10 while stopped too.
#[gpui::test]
fn step_over_goes_to_the_next_line(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at(cx, 0);
    debug.run("debug.step_over");
    debug.wait_stop_at(1);
    assert_eq!(count(&fake, "next"), 1);
    // F10 while stopped, in the stopped file's editor.
    debug.until("the editor", |debug| debug.active_editor().is_some());
    let editor = debug.active_editor().unwrap();
    debug.caret_to(&editor, 1);
    debug.cx.simulate_keystrokes("f10");
    debug.wait_stop_at(2);
    assert_eq!(count(&fake, "next"), 2);
}

/// `debug.step_into` goes into `add`, by F11 while stopped too.
#[gpui::test]
fn step_into_goes_into_the_call(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at(cx, 1);
    debug.run("debug.step_into");
    debug.wait_stop_at(2);
    assert_eq!(count(&fake, "stepIn"), 1);
    debug.cx.simulate_keystrokes("f11");
    debug.wait_stop_at(3);
    assert_eq!(count(&fake, "stepIn"), 2);
}

/// `debug.step_out` leaves `add` for its caller's next line.
#[gpui::test]
fn step_out_returns_to_the_caller(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at(cx, 3);
    debug.run("debug.step_out");
    debug.wait_stop_at(4);
    assert_eq!(count(&fake, "stepOut"), 1);
}

/// `debug.restart` restarts the debuggee, which stops on its breakpoint
/// again.
#[gpui::test]
fn restart_restarts_the_program(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at(cx, 1);
    debug.run("debug.step_over");
    debug.wait_stop_at(2);
    debug.run("debug.restart");
    debug.until("the restart", |_| count(&fake, "restart") == 1);
    debug.wait_stop_at(1);
    assert_eq!(debug.launcher.launched().len(), 1, "the same adapter");
}

/// `debug.run_to_cursor` stops at the caret's line through a hidden
/// breakpoint, which the next stop takes out whatever its reason: one of
/// the user's first, then its own.
#[gpui::test]
fn run_to_cursor_sets_a_one_shot_breakpoint_until_the_next_stop(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at(cx, 0);
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.until("the added breakpoint", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1, 3])
    });
    let editor = debug.open_at(&main, 4);
    let sends = sent_lines(&fake, &main).len();

    // The user's breakpoint on line 3 comes first: the hidden one on line 5
    // goes with that stop, though it's not where it stopped.
    debug.run("debug.run_to_cursor");
    debug.wait_stop_at(2);
    debug.until("the hidden breakpoint taken out", |_| {
        sent_lines(&fake, &main).len() == sends + 2
    });
    assert_eq!(
        sent_lines(&fake, &main)[sends..],
        [vec![1, 3, 5], vec![1, 3]]
    );
    let debugger = debug.debugger();
    let target = debugger.read_with(debug.cx, |debugger, _| {
        debugger.run_to_cursor_target().map(|(_, line)| line)
    });
    assert_eq!(target, None);
    // It was never the user's, nor said of in their breakpoints'.
    assert_eq!(debug.breakpoints(&main), [(1, true), (3, true)]);
    let verified = debugger.read_with(debug.cx, |debugger, _| {
        (debugger.verification(&main).iter())
            .map(|verification| verification.line)
            .collect::<Vec<_>>()
    });
    assert_eq!(verified, [1, 3]);

    // Again from line 3: it stops at the caret's line, its own stop.
    debug.caret_to(&editor, 4);
    debug.run("debug.run_to_cursor");
    debug.wait_stop_at(4);
    debug.until("the hidden breakpoint taken out", |_| {
        sent_lines(&fake, &main).len() == sends + 4
    });
    assert_eq!(
        sent_lines(&fake, &main)[sends + 2..],
        [vec![1, 3, 5], vec![1, 3]]
    );
    assert_eq!(count(&fake, "continue"), 2);
    // Not where it isn't stopped.
    debug.run("debug.continue");
    debug.wait_state("the end", SessionState::is_ended);
}

/// `debug.start_with_args` asks for arguments, then starts the
/// configuration picked with them after its own, which a rerun keeps.
#[gpui::test]
fn start_with_args_adds_the_arguments_typed(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    debug.run("debug.start_with_args");
    let none = tr!("workbench-debug-args-none").to_string();
    assert_eq!(
        debug.command_picker(),
        Some((ARGS_PICKER.0, vec![none.clone()]))
    );
    debug
        .cx
        .simulate_input("--count 'two words' \"say \\\"hi\\\"\"");
    debug.draw();
    let typed = "--count 'two words' \"say \\\"hi\\\"\"";
    let start = tr!("workbench-debug-args-start", args = typed).to_string();
    assert_eq!(debug.command_picker(), Some((ARGS_PICKER.0, vec![start])));
    debug.confirm_command_picker();
    debug.draw();
    let workbench = debug.workbench.clone();
    let picker = debug
        .cx
        .update(|_, cx| workbench.read(cx).debug_picker(cx))
        .expect("the configuration picker shows");
    debug.until("the list", |debug| {
        picker.read_with(debug.cx, |picker, _| picker.delegate.loaded())
    });
    picker.update_in(debug.cx, |picker, window, cx| picker.confirm(0, window, cx));
    let fake = debug.fake(0);
    debug.until("the launch", |_| !requests(&fake, "launch").is_empty());
    let want = ["--count", "two words", "say \"hi\""];
    assert_eq!(
        requests(&fake, "launch")[0]["args"],
        serde_json::json!(want)
    );

    debug.run("debug.rerun_last");
    let rerun = debug.fake(1);
    debug.until("the rerun's launch", |_| {
        !requests(&rerun, "launch").is_empty()
    });
    assert_eq!(
        requests(&rerun, "launch")[0]["args"],
        serde_json::json!(want)
    );

    // A plain start adds none.
    debug.run("debug.stop");
    debug.run("debug.start");
    let picker = debug
        .cx
        .update(|_, cx| workbench.read(cx).debug_picker(cx))
        .expect("the configuration picker shows");
    debug.until("the list", |debug| {
        picker.read_with(debug.cx, |picker, _| picker.delegate.loaded())
    });
    picker.update_in(debug.cx, |picker, window, cx| picker.confirm(0, window, cx));
    let plain = debug.fake(2);
    debug.until("the plain launch", |_| {
        !requests(&plain, "launch").is_empty()
    });
    let args = &requests(&plain, "launch")[0]["args"];
    assert!(args.is_null() || args == &serde_json::json!([]), "{args}");
}

/// The arguments are split as a shell would; an open quote or a trailing
/// backslash leaves them incomplete (review F6), and an escaped backslash
/// is one.
#[test]
fn arguments_split_as_a_shell_would() {
    let split = |text: &str| split_args(text);
    let args = |args: &[&str]| Ok(args.iter().map(ToString::to_string).collect::<Vec<_>>());
    assert_eq!(split(""), args(&[]));
    assert_eq!(split("  a  b "), args(&["a", "b"]));
    assert_eq!(split("'a b' c"), args(&["a b", "c"]));
    assert_eq!(split(r#""a \"b\"" c\ d"#), args(&[r#"a "b""#, "c d"]));
    assert_eq!(split("x''"), args(&["x"]));
    assert_eq!(split("''"), args(&[""]));
    assert_eq!(split(r"a\\ b"), args(&[r"a\", "b"]));
    assert_eq!(split(r"a\\"), args(&[r"a\"]));
    assert_eq!(split("'open"), Err(Incomplete::Quote));
    assert_eq!(split("\"open"), Err(Incomplete::Quote));
    assert_eq!(split(r#""a\"#), Err(Incomplete::Quote));
    assert_eq!(split(r"abc\"), Err(Incomplete::Escape));
    assert_eq!(split(r"\"), Err(Incomplete::Escape));
}

/// Review F6: arguments ending in a backslash say so, and start nothing.
#[gpui::test]
fn a_trailing_backslash_starts_nothing(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    debug.run("debug.start_with_args");
    debug.cx.simulate_input("--name abc\\");
    debug.draw();
    let escape = tr!("workbench-debug-args-trailing-escape").to_string();
    assert_eq!(debug.command_picker(), Some((ARGS_PICKER.0, vec![escape])));
    debug.confirm_command_picker();
    debug.draw();
    assert_eq!(
        debug.command_picker().map(|(id, _)| id),
        Some(ARGS_PICKER.0),
        "still asking"
    );
    let workbench = debug.workbench.clone();
    let configs = debug.cx.update(|_, cx| workbench.read(cx).debug_picker(cx));
    assert!(configs.is_none());
    assert_eq!(debug.launcher.launched().len(), 0);
}

/// `debug.show_session` lists the session with its state; picking it
/// shows where it's stopped.
#[gpui::test]
fn show_session_lists_the_session_and_shows_its_stop(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    debug.run("debug.show_session");
    let none = tr!("workbench-debug-not-running").to_string();
    assert_eq!(debug.toasts(), [(Level::Info, none)]);
    assert_eq!(debug.command_picker(), None);

    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.start("main");
    debug.wait_stop_at(2);
    debug.until("the stopped file", |debug| {
        debug.caret() == Some((main.clone(), 2))
    });
    let editor = debug.active_editor().unwrap();
    debug.caret_to(&editor, 0);
    debug.run("debug.show_session");
    let stopped = tr!("workbench-debug-run-stopped").to_string();
    assert_eq!(
        debug.command_picker(),
        Some((SESSION_PICKER.0, vec![format!("main, {stopped}")]))
    );
    debug.confirm_command_picker();
    debug.until("the stop shown", |debug| {
        debug.caret() == Some((main.clone(), 2))
    });
    assert_eq!(debug.command_picker(), None);
}

/// `debug.toggle_breakpoint` adds a breakpoint at the caret's row, and
/// removes it, by F9 too; with no file it says so.
#[gpui::test]
fn toggle_breakpoint_at_the_caret(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    debug.run("debug.toggle_breakpoint");
    let no_file = tr!("workbench-debug-no-file").to_string();
    assert_eq!(debug.toasts(), [(Level::Info, no_file)]);

    let main = main_path(&debug.root);
    debug.open_at(&main, 2);
    debug.run("debug.toggle_breakpoint");
    assert_eq!(debug.breakpoints(&main), [(3, true)]);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(2);
    assert_eq!(sent_lines(&fake, &main), [vec![3]], "sent as it starts");
    let editor = debug.active_editor().unwrap();
    debug.caret_to(&editor, 2);
    debug.cx.simulate_keystrokes("f9");
    debug.draw();
    assert_eq!(debug.breakpoints(&main), []);
    debug.until("the removal sent", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![])
    });
}

/// `debug.toggle_breakpoint_enabled` disables the caret's breakpoint and
/// enables it again; on a row with none it says so.
#[gpui::test]
fn toggle_breakpoint_enabled_at_the_caret(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    debug.open_at(&main, 2);
    debug.run("debug.toggle_breakpoint_enabled");
    assert_eq!(debug.breakpoints(&main), [(3, false)]);
    debug.run("debug.toggle_breakpoint_enabled");
    assert_eq!(debug.breakpoints(&main), [(3, true)]);
    let editor = debug.active_editor().unwrap();
    debug.caret_to(&editor, 1);
    debug.run("debug.toggle_breakpoint_enabled");
    let none = tr!("workbench-debug-no-breakpoint-here").to_string();
    assert_eq!(debug.toasts(), [(Level::Info, none)]);
    assert_eq!(debug.breakpoints(&main), [(3, true)]);
}

/// `debug.remove_all_breakpoints` clears every file's, and a session is
/// sent the empty sets.
#[gpui::test]
fn remove_all_breakpoints_clears_every_file(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    let other = debug.root.join("src").join("other.rs");
    fs::write(&other, "fn other() {}\n").unwrap();
    debug.toggle(&main, 0);
    debug.toggle(&main, 2);
    debug.toggle(&other, 0);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(0);
    debug.run("debug.remove_all_breakpoints");
    assert_eq!(debug.breakpoints(&main), []);
    assert_eq!(debug.breakpoints(&other), []);
    debug.until("the empty sets", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![])
            && sent_lines(&fake, &other).last() == Some(&vec![])
    });
}

/// Review F1: run to cursor continues only once the adapter has answered
/// its breakpoint's `setBreakpoints`: a reply held back holds the Continue
/// back too.
#[gpui::test]
fn run_to_cursor_waits_for_its_breakpoint_before_continuing(cx: &mut TestAppContext) {
    // The session's own send is the first; run to cursor's the second.
    let (mut debug, fake) = stopped_at_with(cx, 0, |root| Script {
        faults: vec![Fault::new("setBreakpoints", FaultKind::ReplyAfter(Trigger::Manual)).nth(2)],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.open_at(&main, 4);
    debug.run("debug.run_to_cursor");
    debug.until("the hidden breakpoint sent", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1, 5])
    });
    thread::sleep(Duration::from_millis(200));
    debug.draw();
    assert_eq!(count(&fake, "continue"), 0, "not before the reply");
    assert_eq!(debug.state(), Some(SessionState::Stopped));

    fake.release();
    debug.until("the continue", |_| count(&fake, "continue") == 1);
    debug.wait_stop_at(4);
    debug.until("the hidden breakpoint taken out", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1])
    });
}

/// Review F1: a `setBreakpoints` for run to cursor that fails continues
/// nothing: it's said, the hidden breakpoint goes and the user's set is
/// sent again.
#[gpui::test]
fn a_failed_run_to_cursor_breakpoint_continues_nothing(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at_with(cx, 0, |root| Script {
        faults: vec![
            Fault::new("setBreakpoints", FaultKind::Fail("no such file".to_owned())).nth(2),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.open_at(&main, 4);
    debug.run("debug.run_to_cursor");
    debug.until("the user's set sent again", |_| {
        sent_lines(&fake, &main) == [vec![1], vec![1, 5], vec![1]]
    });
    let failed = tr!("workbench-debug-request-failed", name = "main").to_string();
    assert!(
        debug.toasts().contains(&(Level::Warning, failed)),
        "{:?}",
        debug.toasts()
    );
    thread::sleep(Duration::from_millis(100));
    debug.draw();
    assert_eq!(count(&fake, "continue"), 0);
    assert_eq!(debug.state(), Some(SessionState::Stopped));
    let debugger = debug.debugger();
    let target = debugger.read_with(debug.cx, |debugger, _| {
        debugger.run_to_cursor_target().map(|(_, line)| line)
    });
    assert_eq!(target, None);
}

/// Review F4: extra arguments are their configuration's: a fresh start of
/// another doesn't carry them, its reruns (`debug.rerun_last`, a debuggee
/// tab's) add them again, and a fresh start without them drops them.
#[gpui::test]
fn extra_arguments_belong_to_their_configuration(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let args = |fake: &FakeAdapter| requests(fake, "launch")[0]["args"].clone();
    let launched = |debug: &mut Debug, n: usize| {
        let fake = debug.fake(n);
        debug.until("the launch", |_| !requests(&fake, "launch").is_empty());
        fake
    };
    let pick = |debug: &mut Debug, row: usize| {
        let workbench = debug.workbench.clone();
        let picker = debug
            .cx
            .update(|_, cx| workbench.read(cx).debug_picker(cx))
            .expect("the configuration picker shows");
        debug.until("the list", |debug| {
            picker.read_with(debug.cx, |picker, _| picker.delegate.loaded())
        });
        picker.update_in(debug.cx, |picker, window, cx| {
            picker.confirm(row, window, cx)
        });
        debug.draw();
    };
    let none = serde_json::json!([]);

    // `main` with `--a`, then `other` plainly.
    debug.run("debug.start_with_args");
    debug.cx.simulate_input("--a");
    debug.draw();
    debug.confirm_command_picker();
    pick(&mut debug, 0);
    let fake = launched(&mut debug, 0);
    assert_eq!(args(&fake), serde_json::json!(["--a"]));
    debug.run("debug.start");
    pick(&mut debug, 1);
    let fake = launched(&mut debug, 1);
    assert_eq!(args(&fake), none, "other has none");

    // `main`'s debuggee tab reruns it with its own, `other`'s rerun none.
    debug.start("main");
    let fake = launched(&mut debug, 2);
    assert_eq!(args(&fake), serde_json::json!(["--a"]));
    debug.start("other");
    let fake = launched(&mut debug, 3);
    assert_eq!(args(&fake), none);
    debug.run("debug.rerun_last");
    let fake = launched(&mut debug, 4);
    assert_eq!(args(&fake), none, "the last was other");

    // A fresh start of `main` without them drops them.
    let workbench = debug.workbench.clone();
    debug.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.start_debugging("main", window, cx)
        })
    });
    debug.draw();
    let fake = launched(&mut debug, 5);
    assert_eq!(args(&fake), none);
    debug.start("main");
    let fake = launched(&mut debug, 6);
    assert_eq!(args(&fake), none);
}

/// A `stopped` event of the fake's thread, as another stop while stopped
/// would send it.
fn stopped_event() -> Message {
    use fe_dap::protocol::{EventMessage, StoppedBody, event};
    let body = StoppedBody {
        reason: "pause".into(),
        thread_id: Some(fe_dap::fake::THREAD_ID),
        all_threads_stopped: Some(true),
        ..StoppedBody::default()
    };
    Message::Event(EventMessage::new::<event::Stopped>(0, &body))
}

/// Review round 2 F1: a stop that comes while run to cursor waits on its
/// breakpoint's reply ends that run: the reply, once it comes, resumes
/// nothing (the resume is checked against the stop it was asked in as it
/// goes, `Session::resume_at`).
#[gpui::test]
fn run_to_cursor_never_resumes_a_newer_stop(cx: &mut TestAppContext) {
    let (mut debug, fake) = stopped_at_with(cx, 0, |root| Script {
        faults: vec![Fault::new("setBreakpoints", FaultKind::ReplyAfter(Trigger::Manual)).nth(2)],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.open_at(&main, 4);
    debug.run("debug.run_to_cursor");
    debug.until("the hidden breakpoint sent", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1, 5])
    });
    let debugger = debug.debugger();
    let epoch = |debug: &mut Debug| {
        debugger.read_with(debug.cx, |debugger, _| debugger.session().unwrap().epoch())
    };
    let asked = epoch(&mut debug);
    fake.send(stopped_event());
    debug.until("the newer stop", |debug| epoch(debug) > asked);
    fake.release();
    debug.until("its breakpoint taken out", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1])
    });
    thread::sleep(Duration::from_millis(100));
    debug.draw();
    assert_eq!(count(&fake, "continue"), 0);
    assert_eq!(debug.state(), Some(SessionState::Stopped));
}

/// Review round 2 F5: a hidden breakpoint whose removal the adapter
/// rejects stays the session's to clear: the next run to cursor sends its
/// file again and continues only once that's answered, not while it fails.
#[gpui::test]
fn a_rejected_removal_holds_the_next_run_to_cursor(cx: &mut TestAppContext) {
    // The third send takes the first run's breakpoint out of `other.rs`, the
    // fourth again for the second run: both refused.
    let fail = || FaultKind::Fail("busy".to_owned());
    let mut debug = trusted(cx, |root| Script {
        faults: vec![
            Fault::new("setBreakpoints", fail()).nth(3),
            Fault::new("setBreakpoints", fail()).nth(4),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    let other = debug.root.join("src").join("other.rs");
    fs::write(&other, "fn other() {}\n").unwrap();
    debug.toggle(&main, 0);
    debug.toggle(&main, 2);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(0);

    // To `other.rs`, which the program never reaches: the user's breakpoint
    // on line 3 stops it first, and taking the hidden one out fails.
    debug.open_at(&other, 0);
    debug.run("debug.run_to_cursor");
    debug.wait_stop_at(2);
    debug.until("the refused removal", |_| {
        sent_lines(&fake, &other) == [vec![1], vec![]]
    });

    // The next run sends `other.rs` again first; refused again, it doesn't
    // continue, and its own breakpoint goes.
    debug.open_at(&main, 4);
    debug.run("debug.run_to_cursor");
    debug.until("its breakpoint taken out", |_| {
        sent_lines(&fake, &main).last() == Some(&vec![1, 3]) && sent_lines(&fake, &main).len() == 3
    });
    assert_eq!(sent_lines(&fake, &other), [vec![1], vec![], vec![]]);
    thread::sleep(Duration::from_millis(100));
    debug.draw();
    assert_eq!(count(&fake, "continue"), 1, "only the first run's");
    assert_eq!(debug.stopped_row(), Some(2));

    // Once `other.rs` is answered, it runs to the caret.
    debug.run("debug.run_to_cursor");
    debug.wait_stop_at(4);
    assert_eq!(sent_lines(&fake, &other).last(), Some(&vec![]));
    assert_eq!(count(&fake, "continue"), 2);
}

/// Review round 2 F5: a run to cursor that replaces an earlier one's
/// breakpoint in another file waits for that file's removal too, not only
/// its own breakpoint's.
#[gpui::test]
fn a_delayed_removal_holds_the_continue(cx: &mut TestAppContext) {
    // The first run's send (the second) and the removal (the third) are
    // held until the test lets them go.
    let held = || FaultKind::ReplyAfter(Trigger::Manual);
    let (mut debug, fake) = stopped_at_with(cx, 0, |root| Script {
        faults: vec![
            Fault::new("setBreakpoints", held()).nth(2),
            Fault::new("setBreakpoints", held()).nth(3),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    let other = debug.root.join("src").join("other.rs");
    fs::write(&other, "fn other() {}\n").unwrap();
    debug.open_at(&other, 0);
    debug.run("debug.run_to_cursor");
    debug.until("the first run's send", |_| {
        sent_lines(&fake, &other) == [vec![1]]
    });
    debug.open_at(&main, 4);
    debug.run("debug.run_to_cursor");
    debug.until("the removal and the new breakpoint", |_| {
        sent_lines(&fake, &other) == [vec![1], vec![]]
            && sent_lines(&fake, &main).last() == Some(&vec![1, 5])
    });
    thread::sleep(Duration::from_millis(200));
    debug.draw();
    assert_eq!(
        count(&fake, "continue"),
        0,
        "not before the removal's answer"
    );

    fake.release();
    debug.until("the continue", |_| count(&fake, "continue") == 1);
    debug.wait_stop_at(4);
}
