//! Format Document and format on save with an external formatter: a fake
//! [`Runner`] beside the fake language server, chosen by `prefer`.

use std::{
    cell::{Cell, RefCell},
    fs,
    future::{self, Future},
    io,
    path::PathBuf,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_buffer::ResolvedSelection;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent, SettingsStore};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use futures::channel::oneshot;
use gpui::{Entity, Focusable as _, TestAppContext};
use kiln_platform::config::{self, BatchSender, ConfigService};
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::{
    external_format::{RunOutput, RunSpec, Runner},
    lsp_format::FormatOutcome,
    tests::lsp::Lsp,
};

const FORMATTING: &str = "textDocument/formatting";
const RANGE: &str = "textDocument/rangeFormatting";
const TABS: &str = "fn main() {\n\tf();\n\tg();\n\th();\n}\n";
/// What the fake formatter makes of TABS: the rows around `g` respaced.
const SPACED: &str = "fn main() {\n    f();\n\tg();\n    h();\n}\n";
/// What the fake server makes of TABS.
const SERVED: &str = "fn main() {\n\tff();\n\tg();\n\th();\n}\n";

const EXTERNAL: &str = "[formatters.rust]\ncommand = [\"rustfmt\", \"--emit\", \"stdout\"]\n";
const PREFER_LSP: &str =
    "[formatters.rust]\ncommand = [\"rustfmt\", \"--emit\", \"stdout\"]\nprefer = \"lsp\"\n";
const QUICK: &str =
    "[formatters.rust]\ncommand = [\"rustfmt\", \"--emit\", \"stdout\"]\ntimeout_ms = 100\n";

/// What the fake formatter does when run.
enum Mode {
    Output(&'static str),
    Missing,
    Fail(&'static str),
    Hang,
    /// Waits for [`FakeRunner::release`].
    Wait,
}

struct FakeRunner {
    mode: RefCell<Mode>,
    specs: RefCell<Vec<RunSpec>>,
    waiting: RefCell<Option<oneshot::Sender<io::Result<RunOutput>>>>,
    stopped: Rc<Cell<bool>>,
}

impl FakeRunner {
    fn new(mode: Mode) -> Rc<Self> {
        Rc::new(Self {
            mode: RefCell::new(mode),
            specs: RefCell::default(),
            waiting: RefCell::default(),
            stopped: Rc::default(),
        })
    }

    fn set(&self, mode: Mode) {
        *self.mode.borrow_mut() = mode;
    }

    fn calls(&self) -> usize {
        self.specs.borrow().len()
    }

    fn release(&self, answer: io::Result<RunOutput>) {
        let waiting = self.waiting.borrow_mut().take();
        waiting.expect("a run waits").send(answer).unwrap();
    }
}

fn output(stdout: &str) -> RunOutput {
    RunOutput {
        code: Some(0),
        stdout: stdout.into(),
        stderr: Vec::new(),
    }
}

fn failed(stderr: &str) -> RunOutput {
    RunOutput {
        code: Some(1),
        stdout: Vec::new(),
        stderr: stderr.into(),
    }
}

struct SetOnDrop(Rc<Cell<bool>>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

impl Runner for FakeRunner {
    fn run(&self, spec: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>> {
        self.specs.borrow_mut().push(spec);
        let answer = match &*self.mode.borrow() {
            Mode::Output(stdout) => Ok(output(stdout)),
            Mode::Missing => Err(io::ErrorKind::NotFound.into()),
            Mode::Fail(stderr) => Ok(failed(stderr)),
            Mode::Hang => {
                let stopped = SetOnDrop(self.stopped.clone());
                return Box::pin(async move {
                    let _stopped = stopped;
                    future::pending().await
                });
            }
            Mode::Wait => {
                let (sender, answer) = oneshot::channel();
                *self.waiting.borrow_mut() = Some(sender);
                return Box::pin(async move { answer.await.unwrap() });
            }
        };
        Box::pin(future::ready(answer))
    }
}

/// A rust server that formats documents and ranges, answering [`SERVED`].
fn formatter() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn no_formatter() -> Script {
    Script::default()
}

/// The workbench over the fakes, `settings` in force, `src/main.rs` (as TABS)
/// open and focused.
struct Wired {
    s: Lsp,
    editor: Entity<EditorView>,
    runner: Rc<FakeRunner>,
    _settings: (tempfile::TempDir, BatchSender, gpui::Task<()>),
}

fn setup(cx: &mut TestAppContext, rust: Script, settings: &str, mode: Mode) -> Wired {
    setup_with_trust(cx, rust, settings, mode, true)
}

fn setup_with_trust(
    cx: &mut TestAppContext,
    rust: Script,
    settings: &str,
    mode: Mode,
    trusted: bool,
) -> Wired {
    let mut s = Lsp::with_scripts(cx, rust, Script::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, settings).unwrap();
    let store = SettingsStore::load(Some(path), None);
    let (sender, batches) = config::batch_channel();
    let service = s.cx.update(|_, cx| {
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
    });
    let runner = FakeRunner::new(mode);
    let installed = runner.clone();
    (s.workbench).update(&mut s.cx, |w, _| w.format_runner = installed);
    if trusted {
        s.set_trust(true);
    }
    fs::write(s.path("src/main.rs"), TABS).unwrap();
    s.open("src/main.rs");
    if trusted {
        s.fake
            .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some())
            .unwrap();
    }
    let editor = active(&mut s);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    Wired {
        s,
        editor,
        runner,
        _settings: (dir, sender, service),
    }
}

fn active(s: &mut Lsp) -> Entity<EditorView> {
    (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn run(s: &mut Lsp, command: &str) {
    let stack = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let invocation = Invocation::of(command).unwrap().context(stack);
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

fn requests(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// Has the server answer [`SERVED`] from now on.
fn serve(s: &Lsp) {
    let edit = json!({
        "range": {
            "start": { "line": 1, "character": 1 },
            "end": { "line": 1, "character": 2 },
        },
        "newText": "ff",
    });
    s.fake
        .set_reply(FORMATTING, Scripted::now(Reply::Result(json!([edit]))));
}

fn on_disk(s: &Lsp) -> String {
    fs::read_to_string(s.path("src/main.rs")).unwrap()
}

fn format_on_save(s: &mut Lsp) {
    let mut settings =
        s.cx.update(|_, cx| Settings::clone(&config::settings::<Settings>(cx)));
    settings.editor.format_on_save = true;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    (s.workbench).update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

/// `task`'s answer, drawing until it has one.
fn block<T: 'static>(s: &mut Lsp, task: gpui::Task<T>) -> T {
    let answer = Rc::new(RefCell::new(None));
    let slot = answer.clone();
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach()
    });
    s.until("the task", |_| answer.borrow().is_some());
    answer.take().unwrap()
}

#[gpui::test]
fn external_formatter_formats_the_document(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), EXTERNAL, Mode::Output(SPACED));
    // The caret on the row between the two the formatter changes.
    let on_g = TABS.find("g()").unwrap();
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            view.model_mut().set_selections([ResolvedSelection {
                range: on_g..on_g,
                reversed: false,
            }]);
        })
    });

    run(&mut s, "lsp.format_document");
    s.until("the document formatted", |s| text(s, &editor) == SPACED);

    let spec = runner.specs.borrow()[0].clone();
    assert_eq!(spec.program, "rustfmt");
    assert_eq!(spec.stdin.as_deref(), Some(TABS.as_bytes()));
    assert_eq!(spec.cwd, Some(s.path("src")));
    assert!(
        requests(&s, FORMATTING).is_empty(),
        "the server's not asked"
    );
    let caret = editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    let now_on_g = SPACED.find("g()").unwrap();
    assert_eq!(
        caret,
        now_on_g..now_on_g,
        "the caret stays on the same text"
    );
    assert!(toasts(&s).is_empty());

    // Its several hunks are one step to undo.
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);
}

#[gpui::test]
fn prefer_external_falls_back_to_lsp(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), EXTERNAL, Mode::Missing);
    serve(&s);

    // No such program: the server formats, and that's all anyone hears.
    run(&mut s, "lsp.format_document");
    s.until("the server's format", |s| text(s, &editor) == SERVED);
    assert_eq!(runner.calls(), 1);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);

    // It ran and failed: the server formats, and one line says why.
    runner.set(Mode::Fail("\nerror: bad token\nsecond line\n"));
    run(&mut s, "lsp.format_document");
    s.until("the server's format", |s| text(s, &editor) == SERVED);
    assert_eq!(runner.calls(), 2);
    assert_eq!(requests(&s, FORMATTING).len(), 2);
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["rustfmt: error: bad token"]);
}

#[gpui::test]
fn every_source_failing_toasts_once(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), EXTERNAL, Mode::Fail("boom"));
    run(&mut s, "lsp.format_document");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["rustfmt: boom"]);
    assert_eq!(text(&s, &editor), TABS);

    // Nothing to fall back on, so a missing program is worth saying.
    runner.set(Mode::Missing);
    run(&mut s, "lsp.format_document");
    s.until("the toast", |s| {
        toasts(s).len() > 1 || toasts(s)[0] != "rustfmt: boom"
    });
    assert!(
        (toasts(&s).iter()).any(|toast| toast == "rustfmt: program not found"),
        "{:?}",
        toasts(&s)
    );
    assert!(
        !toasts(&s)
            .iter()
            .any(|toast| toast.starts_with("No formatter"))
    );
}

#[gpui::test]
fn prefer_lsp_uses_external_when_lsp_has_no_formatter(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), PREFER_LSP, Mode::Output(SPACED));
    run(&mut s, "lsp.format_document");
    s.until("the formatter's format", |s| text(s, &editor) == SPACED);
    assert_eq!(runner.calls(), 1);
    assert!(toasts(&s).is_empty());
}

#[gpui::test]
fn prefer_lsp_never_runs_external_when_lsp_formats(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), PREFER_LSP, Mode::Output(SPACED));
    serve(&s);
    run(&mut s, "lsp.format_document");
    s.until("the server's format", |s| text(s, &editor) == SERVED);
    assert_eq!(runner.calls(), 0);

    // Nothing to change is still an answer.
    s.fake
        .set_reply(FORMATTING, Scripted::now(Reply::Result(json!([]))));
    run(&mut s, "lsp.format_document");
    s.until("the second request", |s| requests(s, FORMATTING).len() == 2);
    s.draw();
    assert_eq!(runner.calls(), 0);
    assert_eq!(text(&s, &editor), SERVED);
}

#[gpui::test]
fn prefer_lsp_falls_back_when_the_server_fails(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), PREFER_LSP, Mode::Output(SPACED));
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Error {
            code: -32603,
            message: "it broke".into(),
        }),
    );
    run(&mut s, "lsp.format_document");
    s.until("the formatter's format", |s| text(s, &editor) == SPACED);
    assert_eq!(runner.calls(), 1);
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s).len(), 1, "{:?}", toasts(&s));
    assert!(toasts(&s)[0].contains("it broke"), "{:?}", toasts(&s));
}

#[gpui::test]
fn format_on_save_runs_the_external_formatter(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), QUICK, Mode::Wait);
    format_on_save(&mut s);

    // Formatted first, and only then written.
    s.cx.simulate_keystrokes("cmd-s");
    s.until("the run", |_| runner.calls() == 1);
    s.draw();
    assert_eq!(on_disk(&s), TABS, "nothing written while it runs");
    runner.release(Ok(output(SPACED)));
    s.until("the formatted text written", |s| on_disk(s) == SPACED);
    assert_eq!(text(&s, &editor), SPACED);
    assert!(toasts(&s).is_empty());

    // It fails: the text is saved as it is, and nothing is shown.
    runner.set(Mode::Fail("boom"));
    s.cx.simulate_input("x");
    s.cx.simulate_keystrokes("cmd-s");
    let typed = text(&s, &editor);
    assert_ne!(typed, SPACED);
    s.until("the failed format's save", |s| on_disk(s) == typed);
    assert_eq!(runner.calls(), 2);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));

    // It times out: likewise, the run stopped.
    runner.set(Mode::Hang);
    s.cx.simulate_input("y");
    s.cx.simulate_keystrokes("cmd-s");
    let typed = text(&s, &editor);
    s.until("the run", |_| runner.calls() == 3);
    assert!(!runner.stopped.get());
    s.cx.executor().advance_clock(Duration::from_millis(100));
    s.until("the timed-out format's save", |s| on_disk(s) == typed);
    assert!(runner.stopped.get());
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
}

#[gpui::test]
fn a_save_stops_a_formatter_past_the_save_hook_limit(cx: &mut TestAppContext) {
    // Its timeout is the default 3 s: the save still goes on after its second.
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), EXTERNAL, Mode::Hang);
    format_on_save(&mut s);
    s.cx.simulate_input("x");
    s.cx.simulate_keystrokes("cmd-s");
    s.until("the run", |_| runner.calls() == 1);
    s.cx.executor().advance_clock(Duration::from_secs(1));
    let typed = text(&s, &editor);
    s.until("the save", |s| on_disk(s) == typed);
    assert!(runner.stopped.get(), "the run is stopped with the hook");
    assert!(toasts(&s).is_empty());
}

#[gpui::test]
fn save_without_formatting_skips_it(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED));
    format_on_save(&mut s);
    s.cx.simulate_input("x");
    let typed = text(&s, &editor);
    run(&mut s, "workbench.save_without_formatting");
    s.until("the save", |s| on_disk(s) == typed);
    assert_eq!(runner.calls(), 0);
    assert_eq!(text(&s, &editor), typed);
}

#[gpui::test]
fn selection_format_ignores_the_external_formatter(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED));
    run(&mut s, "lsp.format_selection");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No formatter for Rust"]);
    assert_eq!(runner.calls(), 0);
    assert_eq!(text(&s, &editor), TABS);
}

#[gpui::test]
fn selection_format_goes_to_a_server_that_has_one(cx: &mut TestAppContext) {
    let Wired { mut s, runner, .. } = setup(cx, formatter(), EXTERNAL, Mode::Output(SPACED));
    s.fake
        .set_reply(RANGE, Scripted::now(Reply::Result(json!([]))));
    run(&mut s, "lsp.format_selection");
    s.until("the request", |s| requests(s, RANGE).len() == 1);
    s.draw();
    assert_eq!(runner.calls(), 0);
    assert!(toasts(&s).is_empty());
}

#[gpui::test]
fn buffer_changed_while_formatting_drops_the_result(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), EXTERNAL, Mode::Wait);
    let task = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(&editor, cx));
    s.until("the run", |_| runner.calls() == 1);
    s.cx.simulate_input("x");
    let typed = text(&s, &editor);
    assert_ne!(typed, TABS);
    runner.release(Ok(output(SPACED)));
    assert_eq!(block(&mut s, task), Ok(FormatOutcome::Unchanged));
    assert_eq!(text(&s, &editor), typed);
}

#[gpui::test]
fn buffer_changed_before_the_fallback_asks_nothing(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), EXTERNAL, Mode::Wait);
    serve(&s);
    let task = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(&editor, cx));
    s.until("the run", |_| runner.calls() == 1);
    s.cx.simulate_input("x");
    let typed = text(&s, &editor);
    runner.release(Ok(failed("boom")));
    assert_eq!(block(&mut s, task), Ok(FormatOutcome::Unchanged));
    assert!(requests(&s, FORMATTING).is_empty());
    assert_eq!(text(&s, &editor), typed);
}

#[gpui::test]
fn output_with_other_line_endings_changes_nothing(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, no_formatter(), EXTERNAL, Mode::Wait);
    let task = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(&editor, cx));
    s.until("the run", |_| runner.calls() == 1);
    runner.release(Ok(output(&TABS.replace('\n', "\r\n"))));
    assert_eq!(block(&mut s, task), Ok(FormatOutcome::Unchanged));
    assert_eq!(text(&s, &editor), TABS);
}

#[gpui::test]
fn an_untrusted_folder_runs_no_formatter(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup_with_trust(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED), false);
    run(&mut s, "lsp.format_document");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No formatter for Rust"]);
    assert_eq!(runner.calls(), 0);
    assert_eq!(text(&s, &editor), TABS);
}

/// Formats the file at `name`, under the root or not, and has it answer "No
/// formatter" with the runner never called.
fn runs_nothing_for(s: &mut Lsp, runner: &FakeRunner, name: &str) {
    s.open(name);
    let editor = active(s);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    run(s, "lsp.format_document");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(s), ["No formatter for Rust"], "{name}");
    assert_eq!(runner.calls(), 0, "{name}");
    assert_eq!(text(s, &editor), TABS, "{name}");
}

#[gpui::test]
fn a_file_outside_a_trusted_root_runs_no_formatter(cx: &mut TestAppContext) {
    let Wired { mut s, runner, .. } = setup(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED));
    let outside = tempfile::tempdir().unwrap();
    let file: PathBuf = outside.path().canonicalize().unwrap().join("other.rs");
    fs::write(&file, TABS).unwrap();
    runs_nothing_for(&mut s, &runner, file.to_str().unwrap());
}

#[gpui::test]
fn a_symlinked_folder_out_of_a_trusted_root_runs_no_formatter(cx: &mut TestAppContext) {
    let Wired { mut s, runner, .. } = setup(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED));
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("other.rs"), TABS).unwrap();
    std::os::unix::fs::symlink(outside.path(), s.path("link")).unwrap();
    runs_nothing_for(&mut s, &runner, "link/other.rs");
}

#[gpui::test]
fn a_symlinked_file_out_of_a_trusted_root_runs_no_formatter(cx: &mut TestAppContext) {
    let Wired { mut s, runner, .. } = setup(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED));
    let outside = tempfile::tempdir().unwrap();
    let real = outside.path().join("real.rs");
    fs::write(&real, TABS).unwrap();
    std::os::unix::fs::symlink(&real, s.path("src/link.rs")).unwrap();
    runs_nothing_for(&mut s, &runner, "src/link.rs");
}

#[gpui::test]
fn a_path_up_out_of_a_trusted_root_runs_no_formatter(cx: &mut TestAppContext) {
    let Wired { mut s, runner, .. } = setup(cx, no_formatter(), EXTERNAL, Mode::Output(SPACED));
    let root = s.path("src").parent().unwrap().to_owned();
    let sibling = tempfile::tempdir_in(root.parent().unwrap()).unwrap();
    fs::write(sibling.path().join("other.rs"), TABS).unwrap();
    let name = sibling.path().file_name().unwrap().to_str().unwrap();
    runs_nothing_for(&mut s, &runner, &format!("../{name}/other.rs"));
}

#[test]
fn folders_are_resolved_past_symlinks_and_parents() {
    use crate::lsp_format::formatter_folders;
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    let (trusted, outside) = (dir.join("trusted"), dir.join("outside"));
    fs::create_dir_all(&trusted).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("real.rs"), "").unwrap();
    std::os::unix::fs::symlink(&outside, trusted.join("folder")).unwrap();
    std::os::unix::fs::symlink(outside.join("real.rs"), trusted.join("file.rs")).unwrap();
    std::os::unix::fs::symlink(outside.join("none"), trusted.join("dangling.rs")).unwrap();

    let folders = |path: PathBuf| formatter_folders(Some(&path), None);
    assert_eq!(
        folders(trusted.join("folder/real.rs")).unwrap(),
        [outside.clone(), outside.clone()]
    );
    assert_eq!(
        folders(trusted.join("../outside/real.rs")).unwrap(),
        [outside.clone(), outside.clone()]
    );
    assert_eq!(
        folders(trusted.join("file.rs")).unwrap(),
        [trusted.clone(), outside.clone()]
    );
    // A new file, by its folder; a dangling link or a missing folder, not at all.
    assert_eq!(
        folders(trusted.join("new.rs")).unwrap(),
        [trusted.clone(), trusted.clone()]
    );
    assert_eq!(folders(trusted.join("dangling.rs")), None);
    assert_eq!(folders(trusted.join("gone/new.rs")), None);

    // Unsaved: where it runs, the root or else this process's folder.
    let link = trusted.join("folder");
    assert_eq!(
        formatter_folders(None, Some(&link)).unwrap(),
        std::slice::from_ref(&outside)
    );
    let here = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(formatter_folders(None, None).unwrap(), [here]);
}

#[gpui::test]
fn a_rootless_unsaved_buffer_needs_the_process_folder_trusted(cx: &mut TestAppContext) {
    let mut s = Lsp::rootless(cx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, EXTERNAL).unwrap();
    let store = SettingsStore::load(Some(path), None);
    let (_sender, batches) = config::batch_channel();
    let _service = s.cx.update(|_, cx| {
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
    });
    let runner = FakeRunner::new(Mode::Output("formatted\n"));
    let installed = runner.clone();
    (s.workbench).update(&mut s.cx, |w, _| w.format_runner = installed);
    run(&mut s, "workbench.new_file");
    let unsaved = active(&mut s);
    s.cx.update(|window, cx| window.focus(&unsaved.focus_handle(cx), cx));
    s.cx.update(|_, cx| unsaved.update(cx, |view, cx| view.set_language(Some("rust"), cx)));
    s.cx.simulate_input("x");
    s.until("the language", |s| {
        unsaved.read_with(&s.cx, |view, _| view.syntax().is_some())
    });

    // It would inherit this process's folder, which no one has trusted.
    run(&mut s, "lsp.format_document");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No formatter for Rust"]);
    assert_eq!(runner.calls(), 0);

    let here = std::env::current_dir().unwrap().canonicalize().unwrap();
    s.cx.update(|_, cx| kiln_shell::trust::decide(&here, true, cx));
    run(&mut s, "lsp.format_document");
    s.until("the format", |s| text(s, &unsaved) == "formatted\n");
    assert_eq!(runner.specs.borrow()[0].cwd, None);
}

#[gpui::test]
fn untrusting_while_the_server_answers_runs_no_formatter(cx: &mut TestAppContext) {
    let Wired {
        mut s,
        editor,
        runner,
        ..
    } = setup(cx, formatter(), PREFER_LSP, Mode::Output(SPACED));
    s.fake.set_reply(FORMATTING, Scripted::now(Reply::Never));
    run(&mut s, "lsp.format_document");
    s.until("the request", |s| requests(s, FORMATTING).len() == 1);
    s.until("the timer", |s| {
        s.clock.sleeping().contains(&Duration::from_secs(3))
    });

    // The request ends in a failure, which would otherwise start the fallback.
    s.set_trust(false);
    s.clock.advance(Duration::from_secs(3));
    s.until("the failure shown", |s| !toasts(s).is_empty());
    assert_eq!(runner.calls(), 0);
    assert_eq!(text(&s, &editor), TABS);
}

#[gpui::test]
fn an_unsaved_buffer_gets_a_placeholder_under_the_root(cx: &mut TestAppContext) {
    let settings =
        "[formatters.rust]\ncommand = [\"prettier\", \"--stdin-filepath\", \"{file}\"]\n";
    let Wired { mut s, runner, .. } =
        setup(cx, no_formatter(), settings, Mode::Output("formatted\n"));
    run(&mut s, "workbench.new_file");
    let unsaved = active(&mut s);
    s.cx.update(|window, cx| window.focus(&unsaved.focus_handle(cx), cx));
    s.cx.update(|_, cx| unsaved.update(cx, |view, cx| view.set_language(Some("rust"), cx)));
    s.cx.simulate_input("x");
    s.until("the language", |s| {
        unsaved.read_with(&s.cx, |view, _| view.syntax().is_some())
    });
    run(&mut s, "lsp.format_document");
    s.until("the format", |s| text(s, &unsaved) == "formatted\n");
    let spec = runner.specs.borrow()[0].clone();
    assert_eq!(spec.args[1], s.path("untitled.rs").into_os_string());
    assert_eq!(spec.stdin.as_deref(), Some(&b"x"[..]));
}
