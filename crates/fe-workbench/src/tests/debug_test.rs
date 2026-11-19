//! Debug Test at Cursor (`debug_test.rs`, debugger card D13) in a headless
//! workbench over a real folder, against fe-dap's fake adapter, which the
//! host's launcher starts in-process. The terminal panel's sessions are
//! fake, so neither cargo nor a test runs: each test hands the build's tab
//! the output cargo would print, and the exit it wants.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_dap::{
    AdapterSpec, AdapterTransport, HostOs, Message, SystemClock,
    fake::{FakeAdapter, FakeLauncher, Script},
};
use fe_gpui::{EditorView, Lens, commands::RunCommand};
use fe_runlog::TestId;
use fe_terminal_view::TerminalViewEvent;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;
use serde_json::{Value, json};

use crate::{
    Launch, TerminalPanel, Workbench,
    debug_session::{DebugHost, debugger_of, set_debug_host},
    debug_test::{build_argv, go_needs_delve},
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    task_run::FakeTaskOutput,
    test_commands::{ConsumedTests, ResolveGates},
    test_run_view::{DEBUG_TEST_SELECTOR, SLOT_SELECTOR, TestRunView},
    test_runnables::{CargoTarget, ResolvedTest, TestFramework},
    test_runner::{TestOutcome, TestResult, Tracks, set_history_dir},
    tests::open,
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

/// A library with two tests in its `tests` module, its `#[cfg(test)]` on
/// row 4: `first`'s `#[test]` on row 6, `adds`'s on row 9.
const LIB: &str = "\
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn first() {}

    #[test]
    fn adds() {
        assert_eq!(super::add(1, 2), 3);
    }
}
";

/// No language server starts.
const NO_SERVERS: &str = "[lsp.servers.rust-analyzer]\nenabled = false\n\n\
                          [lsp.servers.pyright]\nenabled = false\n\n\
                          [lsp.servers.gopls]\nenabled = false\n";

struct Project<'a> {
    _dir: tempfile::TempDir,
    _state: tempfile::TempDir,
    root: PathBuf,
    workbench: Entity<Workbench>,
    launcher: Arc<FakeLauncher>,
    cx: &'a mut VisualTestContext,
}

/// A trusted workbench on `files` under a fresh root, `open` showing in an
/// editor if given, with the fake as the host's Rust adapter and as
/// debugpy.
fn project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
    open_file: Option<&str>,
) -> Project<'a> {
    // The session's thread wakes the workbench's event loop.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (name, text) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let settings = fe_config::project_settings_path(&root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, NO_SERVERS).unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut script = Script::default();
    script.program.path = root.join("src/lib.rs").to_string_lossy().into_owned();
    let launcher = Arc::new(FakeLauncher::new(script));
    let fake = |name: &str, language: &str| AdapterSpec {
        types: vec![name.into()],
        languages: vec![language.into()],
        transport: AdapterTransport::Port(1),
        ..AdapterSpec::new(name, name)
    };
    let host = DebugHost {
        launcher: Some(launcher.clone()),
        clock: Arc::new(SystemClock),
        specs: vec![fake("fake", "rust"), fake("debugpy", "python")],
        ptrace_scope: Arc::new(|| None),
        login: None,
        os: HostOs::Linux,
    };
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(Disk)));
        set_history_dir(cx, state.path().to_owned());
    });
    let file = open_file.map(|name| {
        let path = root.join(name);
        let text = fs::read_to_string(&path).unwrap();
        (path, Some(Buffer::new(&text).into()))
    });
    let launch = Launch {
        root: Some(root.clone()),
        file,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| {
        set_debug_host(cx, host);
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    let mut project = Project {
        _dir: dir,
        _state: state,
        root,
        workbench,
        launcher,
        cx,
    };
    // The editor's parse catches up.
    for _ in 0..3 {
        project.draw();
    }
    project
}

impl Project<'_> {
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

    fn editor(&mut self) -> Entity<EditorView> {
        self.workbench
            .read_with(self.cx, |workbench, cx| {
                workbench.active_editor(cx).cloned()
            })
            .expect("an editor shows")
    }

    /// Puts the caret at the start of `text`'s first `needle`.
    fn caret_at(&mut self, needle: &str) {
        let editor = self.editor();
        editor.update(self.cx, |view, _| {
            let at = view.model().text().to_string().find(needle).unwrap();
            view.model_mut().set_cursor(at);
        });
        self.draw();
    }

    /// Runs workbench command `invocation`, as a key in the editor would.
    fn dispatch(&mut self, invocation: Invocation) {
        let context = ContextStack::new()
            .with(ContextFrame::new("workbench"))
            .with(ContextFrame::new("editor"));
        let run = RunCommand(invocation.context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        self.draw();
    }

    fn run(&mut self, id: &str) {
        self.dispatch(Invocation::of(id).unwrap());
    }

    fn terminal(&mut self) -> Entity<TerminalPanel> {
        self.workbench
            .read_with(self.cx, |workbench, _| workbench.terminal().clone())
    }

    /// What each terminal session was asked to run, and where, in start
    /// order.
    fn started(&mut self) -> Vec<(Vec<String>, PathBuf)> {
        let terminal = self.terminal();
        terminal.read_with(self.cx, |terminal, _| {
            (terminal.started_options.iter())
                .map(|options| {
                    let (program, args) = options.shell.clone().expect("a task's command");
                    ([vec![program], args].concat(), options.cwd.clone())
                })
                .collect()
        })
    }

    /// Ends the active terminal tab's fake session with `code`, `output`
    /// its output.
    fn exit_active(&mut self, output: String, code: i32) {
        self.cx
            .update(|_, cx| cx.set_global(FakeTaskOutput(output)));
        let terminal = self.terminal();
        let view = terminal
            .read_with(self.cx, |terminal, _| terminal.view().cloned())
            .expect("a tab");
        view.update(self.cx, |_, cx| {
            cx.emit(TerminalViewEvent::Exited(Some(code)))
        });
        self.draw();
    }

    /// The `n`th fake launched, from 0, and as which adapter.
    fn fake(&mut self, n: usize) -> (String, FakeAdapter) {
        let launcher = self.launcher.clone();
        self.until(&format!("launch {n}"), |_| launcher.launched().len() > n);
        let adapter = launcher.wait_for_launch(n);
        (launcher.launched()[n].0.name.clone(), adapter)
    }

    /// The fake's `launch` request's arguments, once it has had it.
    fn launch_arguments(&mut self, fake: &FakeAdapter) -> Value {
        self.until("the launch request", |_| !launches(fake).is_empty());
        launches(fake).remove(0)
    }

    /// The resolved tests Run and Debug consumed.
    fn consumed(&mut self) -> Vec<(&'static str, ResolvedTest)> {
        self.cx.update(|_, cx| {
            (cx.try_global::<ConsumedTests>()).map_or_else(Vec::new, |consumed| consumed.0.clone())
        })
    }

    /// Shows the Runs tool window with a recorded run in which `id`
    /// failed, its row selected.
    fn fail(&mut self, id: TestId) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let show = RunCommand(Invocation::of("runs.show").unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(show), cx));
        self.draw();
        let leaf = id.name.rsplit("::").next().unwrap().to_owned();
        let result = TestResult {
            id,
            outcome: TestOutcome::Failed,
            output: "thread 'tests::adds' panicked at src/lib.rs:12:9:\nboom\n".to_owned(),
            secs: Some(0.01),
        };
        let workbench = self.workbench.clone();
        workbench.update(self.cx, |workbench, cx| {
            workbench.record_test_run("cargo test", vec![result], Tracks::default(), cx)
        });
        self.draw();
        let panel = workbench.read_with(self.cx, |wb, _| wb.runs_panel().clone());
        panel.update(self.cx, |panel, cx| {
            panel.list().update(cx, |list, cx| list.select(0, cx))
        });
        self.draw();
        let view: Entity<TestRunView> = panel.read_with(self.cx, |panel, _| panel.detail().clone());
        let tree = view.read_with(self.cx, |view, _| view.tree().clone());
        let at = tree.read_with(self.cx, |tree, _| {
            let rows = tree.delegate().rows();
            rows.iter().position(|row| row.label.as_ref() == leaf)
        });
        tree.update(self.cx, |tree, cx| tree.select(at.expect("a row"), cx));
        self.draw();
    }

    /// Clicks the failure detail's Debug this test, which shows in it.
    fn click_debug_this_test(&mut self) {
        let button = self.cx.debug_bounds(DEBUG_TEST_SELECTOR).expect("drawn");
        let slot = self.cx.debug_bounds(SLOT_SELECTOR).expect("drawn");
        assert!(slot.contains(&button.center()), "{button:?} in {slot:?}");
        self.cx.simulate_click(button.center(), Default::default());
        self.draw();
    }

    /// Each toast showing's message.
    fn toasts(&mut self) -> Vec<String> {
        let layer = self
            .workbench
            .read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| entry.toast.message.to_string())
                .collect()
        })
    }
}

/// The arguments of each `launch` request `fake` got.
fn launches(fake: &FakeAdapter) -> Vec<Value> {
    (fake.received().into_iter())
        .filter_map(|message| match message {
            Message::Request(request) if request.command == "launch" => {
                Some(request.arguments.unwrap_or(Value::Null))
            }
            _ => None,
        })
        .collect()
}

/// Cargo's JSON messages for a build of `package` at `manifest` whose
/// library's test executable is `executable`: its dependency's and its
/// library's own non-test builds first, and a closing line that isn't an
/// artifact.
fn cargo_output(package: &str, manifest: &Path, executable: &Path) -> String {
    let manifest = manifest.to_string_lossy();
    let library = |test: bool, executable: Value| {
        json!({
            "reason": "compiler-artifact",
            "package_id": format!("path+file://{}#0.1.0", manifest.trim_end_matches("/Cargo.toml")),
            "manifest_path": manifest,
            "target": {"kind": ["lib"], "name": package},
            "profile": {"test": test},
            "executable": executable,
        })
    };
    let dependency = json!({
        "reason": "compiler-artifact",
        "manifest_path": "/registry/dep/Cargo.toml",
        "target": {"kind": ["lib"], "name": "dep"},
        "profile": {"test": false},
        "executable": null,
    });
    format!(
        "   Compiling {package} v0.1.0\n{dependency}\n{}\n{}\n{}\n",
        library(false, Value::Null),
        library(
            true,
            Value::String(executable.to_string_lossy().into_owned())
        ),
        r#"{"reason":"build-finished","success":true}"#
    )
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// C1 (§7): a Rust test in a workspace member builds that member's library
/// tests with `--no-run` and cargo's JSON messages, where Run would run
/// them (the workspace), and debugs the test executable cargo named, with
/// `[name, "--exact", "--nocapture"]`, in the member's directory (the
/// artifact's `manifest_path`'s) with `CARGO_MANIFEST_DIR` set to it.
#[gpui::test]
fn a_rust_test_in_a_workspace_member_launches_its_test_binary(cx: &mut TestAppContext) {
    let workspace = "[workspace]\nmembers = [\"member\"]\n";
    let member = "[package]\nname = \"member\"\nversion = \"0.1.0\"\n";
    let mut project = project(
        cx,
        &[
            ("Cargo.toml", workspace),
            ("member/Cargo.toml", member),
            ("member/src/lib.rs", LIB),
        ],
        Some("member/src/lib.rs"),
    );
    project.caret_at("assert_eq!");
    project.run("debug.test_at_cursor");

    let build = strings(&[
        "cargo",
        "test",
        "-p",
        "member",
        "--lib",
        "--no-run",
        "--message-format=json-render-diagnostics",
    ]);
    assert_eq!(project.started(), [(build, project.root.clone())]);
    let member_dir = project.root.join("member");
    let executable = project.root.join("target/debug/deps/member-0123abcd");
    let output = cargo_output("member", &member_dir.join("Cargo.toml"), &executable);
    project.exit_active(output, 0);

    let (adapter, fake) = project.fake(0);
    assert_eq!(adapter, "fake", "the Rust default adapter");
    let launch = project.launch_arguments(&fake);
    assert_eq!(launch["program"], executable.to_string_lossy().as_ref());
    assert_eq!(
        launch["args"],
        json!(["tests::adds", "--exact", "--nocapture"])
    );
    let member_dir = member_dir.to_string_lossy();
    assert_eq!(launch["cwd"], member_dir.as_ref(), "not the workspace");
    assert_eq!(launch["env"]["CARGO_MANIFEST_DIR"], member_dir.as_ref());
}

/// C2 (§7): a pytest test debugs through debugpy running the `pytest`
/// module on its node id, in the root, as Run runs pytest.
#[gpui::test]
fn a_pytest_node_id_launches_debugpy_with_module_pytest(cx: &mut TestAppContext) {
    let tests = "def helper():\n    pass\n\n\nclass TestMath:\n    def test_adds(self):\n        assert 1 + 2 == 3\n";
    let mut project = project(
        cx,
        &[("tests/test_math.py", tests)],
        Some("tests/test_math.py"),
    );
    project.caret_at("assert 1");
    project.run("debug.test_at_cursor");

    assert_eq!(project.started(), [], "no build");
    let (adapter, fake) = project.fake(0);
    assert_eq!(adapter, "debugpy");
    let launch = project.launch_arguments(&fake);
    assert_eq!(launch["module"], "pytest");
    assert_eq!(
        launch["args"],
        json!(["tests/test_math.py::TestMath::test_adds"])
    );
    assert_eq!(launch["cwd"], project.root.to_string_lossy().as_ref());
    assert_eq!(launch.get("program"), None);
}

/// C3 (§7, §13): `tests.run_at_cursor` and `debug.test_at_cursor` consume
/// one resolved test from `test_runnables.rs`: Run runs its program and
/// arguments in its directory, as it did before the seam, and Debug builds
/// from its framework.
#[gpui::test]
fn run_and_debug_resolve_the_same_test(cx: &mut TestAppContext) {
    let package = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";
    let mut project = project(
        cx,
        &[("Cargo.toml", package), ("src/lib.rs", LIB)],
        Some("src/lib.rs"),
    );
    project.caret_at("assert_eq!");
    project.run("tests.run_at_cursor");
    project.until("the run", |project| project.consumed().len() == 1);
    project.run("debug.test_at_cursor");
    project.until("the debug", |project| project.consumed().len() == 2);

    let consumed = project.consumed();
    let [("run", run), ("debug", debug)] = consumed.as_slice() else {
        panic!("Run then Debug, got {consumed:?}");
    };
    assert_eq!(run, debug, "one resolved test");
    assert_eq!(
        run.framework,
        TestFramework::Cargo {
            package: "p".to_owned(),
            target: Some(CargoTarget::Lib),
            name: "tests::adds".to_owned(),
            exact: true,
        }
    );
    assert_eq!(run.cwd, project.root);
    let started = project.started();
    // Run's command line, unchanged by the seam.
    let ran = [vec![run.program.clone()], run.args.clone()].concat();
    assert_eq!(
        ran.join(" "),
        "cargo test -p p tests::adds -- --exact --nocapture"
    );
    assert_eq!(started[0], (ran, run.cwd.clone()));
    // Debug's build, from the same test's framework.
    assert_eq!(
        started[1],
        (build_argv("p", &CargoTarget::Lib), run.cwd.clone())
    );
}

/// C4 (§11 D13): a Go test is refused until D19 brings delve's test mode.
#[gpui::test]
fn a_go_test_is_refused_until_delve(cx: &mut TestAppContext) {
    let test = "package calc\n\nimport \"testing\"\n\nfunc TestAdds(t *testing.T) {\n\tif 1+2 != 3 {\n\t\tt.Fatal(\"sum\")\n\t}\n}\n";
    let mut project = project(
        cx,
        &[("go.mod", "module calc\n"), ("calc_test.go", test)],
        Some("calc_test.go"),
    );
    project.caret_at("if 1+2");
    project.run("debug.test_at_cursor");
    project.until("the refusal", |project| {
        project.toasts().contains(&go_needs_delve())
    });
    assert_eq!(go_needs_delve(), "Go tests need delve");
    let consumed = project.consumed();
    assert!(
        matches!(
            consumed.as_slice(),
            [("debug", ResolvedTest { framework: TestFramework::GoTest { name, .. }, .. })]
                if name == "TestAdds"
        ),
        "{consumed:?}"
    );
    assert_eq!(project.started(), [], "nothing runs");
    assert!(project.launcher.launched().is_empty(), "no session");
}

/// C5 (§13): a test's lens reads `▷ Run test · Debug`, and its Debug item
/// debugs the test under it, wherever the caret is.
#[gpui::test]
fn the_lens_reads_run_test_and_debug(cx: &mut TestAppContext) {
    let package = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";
    let mut project = project(
        cx,
        &[("Cargo.toml", package), ("src/lib.rs", LIB)],
        Some("src/lib.rs"),
    );
    project.caret_at("fn first");
    let editor = project.editor();
    let lenses: Vec<Lens> = editor.update(project.cx, |editor, _| {
        let snapshot = editor.model().snapshot().clone();
        (editor.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .collect()
    });
    let adds = (lenses.iter())
        .find(|lens| lens.row == 9)
        .expect("a lens over adds");
    let titles: Vec<String> = (adds.items.iter())
        .map(|item| item.title.to_string())
        .collect();
    assert_eq!(titles.join(" · "), "▷ Run test · Debug");
    assert_eq!(
        titles[1],
        tr!("workbench-debug-test-lens").to_string(),
        "from the catalogue"
    );
    // The module's lens has no Debug.
    let module = lenses
        .iter()
        .find(|lens| lens.row == 4)
        .expect("the module's");
    assert_eq!(module.items.len(), 1);

    // As drawn: the items `·` separated.
    project.draw();
    let drawn = editor.read_with(project.cx, |editor, _| {
        editor.drawn_annotations().expect("drawn").lenses.clone()
    });
    let texts: Vec<&str> = drawn.iter().map(|text| text.text.as_ref()).collect();
    assert_eq!(
        texts,
        [
            "▷ Run tests",
            "▷ Run test",
            " · ",
            "Debug",
            "▷ Run test",
            " · ",
            "Debug"
        ]
    );

    let debug = adds.items[1].invocation.clone().expect("a click target");
    assert_eq!(debug.command.to_string(), "debug.test_at_cursor");
    assert_eq!(debug.args.get("row").and_then(Value::as_u64), Some(9));
    project.dispatch(debug.source(Source::Mouse));
    project.until("the debug", |project| project.consumed().len() == 1);
    let (by, test) = project.consumed().remove(0);
    assert_eq!(by, "debug");
    assert!(
        matches!(&test.framework, TestFramework::Cargo { name, .. } if name == "tests::adds"),
        "the lens's test, not the caret's: {test:?}"
    );
}

/// C5 (§13): the test failure detail's Debug this test debugs the failed
/// test, found by its function as Open test finds it.
#[gpui::test]
fn debug_this_test_in_a_failure_opens_a_debug_session(cx: &mut TestAppContext) {
    let package = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";
    let mut project = project(cx, &[("Cargo.toml", package), ("src/lib.rs", LIB)], None);
    project.fail(TestId::new("p", "tests::adds"));

    // The button, in the failure's detail, named from the catalogue.
    let label = tr!("workbench-debug-test-failure").to_string();
    assert_eq!(label, "Debug this test");
    let nodes = project.cx.update(|window, cx| fe_ui::nodes(window, cx));
    assert!(
        (nodes.iter()).any(|node| node.role == fe_ui::Role::Button && node.label == label),
        "an accessible button named {label}"
    );
    project.click_debug_this_test();
    project.until("the build", |project| !project.started().is_empty());

    let (argv, _) = project.started().remove(0);
    assert_eq!(argv, build_argv("p", &CargoTarget::Lib));
    let executable = project.root.join("target/debug/deps/p-0123abcd");
    let output = cargo_output("p", &project.root.join("Cargo.toml"), &executable);
    project.exit_active(output, 0);
    let (_, fake) = project.fake(0);
    let launch = project.launch_arguments(&fake);
    assert_eq!(launch["program"], executable.to_string_lossy().as_ref());
    assert_eq!(
        launch["args"],
        json!(["tests::adds", "--exact", "--nocapture"])
    );
    let workbench = project.workbench.clone();
    let debugger = (project.cx)
        .update(|_, cx| debugger_of(&workbench, cx))
        .expect("the window's debugger");
    let configuration = debugger.read_with(project.cx, |debugger, _| {
        debugger.configuration().map(str::to_owned)
    });
    let label = tr!("workbench-lsp-tests-task-label", name = "adds").to_string();
    assert_eq!(configuration, Some(label), "a session of the test's own");
}

/// T1856 F1: a failed test's name doesn't say its package or target, so
/// Debug this test debugs the place whose resolved package, target and name
/// are the failed test's: `tests::adds` of workspace member `b`, not of `a`
/// found first, and `a`'s integration test `adds`, not its library's. A
/// test the folder doesn't have, by its package, debugs nothing.
#[gpui::test]
fn debug_this_test_debugs_the_failed_tests_own_package_and_target(cx: &mut TestAppContext) {
    let workspace = "[workspace]\nmembers = [\"a\", \"b\"]\n";
    let manifest = |name: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
    let (a, b) = (manifest("a"), manifest("b"));
    let api = "#[test]\nfn adds() {}\n";
    let mut project = project(
        cx,
        &[
            ("Cargo.toml", workspace),
            ("a/Cargo.toml", &a),
            ("a/src/lib.rs", LIB),
            ("a/tests/api.rs", api),
            ("b/Cargo.toml", &b),
            ("b/src/lib.rs", LIB),
        ],
        None,
    );
    /// The build Debug this test on the failed `id` starts.
    fn debugged(project: &mut Project<'_>, id: TestId) -> Vec<String> {
        let before = project.started().len();
        project.fail(id);
        project.click_debug_this_test();
        project.until("the build", |project| project.started().len() > before);
        project.started().pop().unwrap().0
    }
    assert_eq!(
        debugged(&mut project, TestId::new("b", "tests::adds")),
        build_argv("b", &CargoTarget::Lib)
    );
    assert_eq!(
        debugged(&mut project, TestId::new("a", "tests::adds")),
        build_argv("a", &CargoTarget::Lib)
    );
    assert_eq!(
        debugged(&mut project, TestId::new("a::api", "adds")),
        build_argv("a", &CargoTarget::Test("api".to_owned()))
    );

    let started = project.started().len();
    project.fail(TestId::new("c", "tests::adds"));
    project.click_debug_this_test();
    let missing = tr!(
        "workbench-debug-test-not-found",
        name = "tests::adds",
        binary = "c"
    )
    .to_string();
    project.until("the refusal", |project| project.toasts().contains(&missing));
    assert_eq!(project.started().len(), started, "nothing built");
}

/// T1856 F2: a debug test's generation is its invocation's: an older one
/// that resolves after a newer one launches nothing.
#[gpui::test]
fn an_older_debug_test_resolved_late_launches_nothing(cx: &mut TestAppContext) {
    let package = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";
    let mut project = project(
        cx,
        &[("Cargo.toml", package), ("src/lib.rs", LIB)],
        Some("src/lib.rs"),
    );
    let (first, first_gate) = async_channel::bounded::<()>(1);
    let (second, second_gate) = async_channel::bounded::<()>(1);
    project.cx.update(|_, cx| {
        cx.set_global(ResolveGates(
            [first_gate, second_gate].into_iter().collect(),
        ))
    });
    project.caret_at("fn first");
    project.run("debug.test_at_cursor");
    project.caret_at("assert_eq!");
    project.run("debug.test_at_cursor");

    // The newer, `adds`, resolves first and builds.
    second.try_send(()).unwrap();
    project.until("the newer's build", |project| project.started().len() == 1);
    // The older, `first`, resolves after it: dropped.
    first.try_send(()).unwrap();
    project.until("the older's resolution", |_| first.is_empty());
    for _ in 0..5 {
        project.draw();
    }
    let consumed = project.consumed();
    assert!(
        matches!(
            consumed.as_slice(),
            [("debug", ResolvedTest { framework: TestFramework::Cargo { name, .. }, .. })]
                if name == "tests::adds"
        ),
        "only the newer was debugged: {consumed:?}"
    );
    assert_eq!(project.started().len(), 1);
}

/// T1856 F2: an ordinary configuration started while a test builds moves
/// the debug test on: the build's end doesn't replace that session.
#[gpui::test]
fn a_session_started_during_a_test_build_is_not_replaced(cx: &mut TestAppContext) {
    let package = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";
    let launch = "[[launch]]\nname = \"main\"\nadapter = \"fake\"\n\
                  program = \"${workspaceFolder}/target/debug/main\"\n";
    let mut project = project(
        cx,
        &[
            ("Cargo.toml", package),
            ("src/lib.rs", LIB),
            (".tenmoku/launch.toml", launch),
        ],
        Some("src/lib.rs"),
    );
    project.caret_at("assert_eq!");
    project.run("debug.test_at_cursor");
    project.until("the build", |project| project.started().len() == 1);
    let terminal = project.terminal();
    let build = terminal
        .read_with(project.cx, |terminal, _| terminal.view().cloned())
        .expect("the build's tab");

    // The ordinary configuration, as the run widget's Debug starts it.
    let args = Value::Object(
        [("configuration".to_owned(), json!("main"))]
            .into_iter()
            .collect(),
    );
    project.dispatch(Invocation::of("debug.start").unwrap().args(args));
    let (_, fake) = project.fake(0);
    project.launch_arguments(&fake);

    // The test's build ends after it.
    let executable = project.root.join("target/debug/deps/p-0123abcd");
    let output = cargo_output("p", &project.root.join("Cargo.toml"), &executable);
    project
        .cx
        .update(|_, cx| cx.set_global(FakeTaskOutput(output)));
    build.update(project.cx, |_, cx| {
        cx.emit(TerminalViewEvent::Exited(Some(0)))
    });
    for _ in 0..10 {
        project.draw();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(project.launcher.launched().len(), 1, "no test session");
    let workbench = project.workbench.clone();
    let debugger = (project.cx)
        .update(|_, cx| debugger_of(&workbench, cx))
        .expect("the window's debugger");
    let configuration = debugger.read_with(project.cx, |debugger, _| {
        debugger.configuration().map(str::to_owned)
    });
    assert_eq!(configuration.as_deref(), Some("main"));
}
