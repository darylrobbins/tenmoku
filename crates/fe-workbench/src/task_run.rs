//! Running a project's task ([`crate::task_defs`]): in a tab of its own on the
//! terminal panel, whose output, rerun and exit report are the panel's
//! (`TerminalPanel::add_task`). The commands that get here are
//! [`crate::task_commands`]'s.
//!
//! As a task's session exits, its output goes through its problem matcher
//! ([`crate::task_problems`]) and the files it names are read, off the main
//! thread, and the Problems panel
//! lists what it found as the tab's source: replacing what its last run found,
//! until the tab closes. Two tabs of one label are two sources. A run started
//! by [`Workbench::run_task_then`] hands the same problems to its caller too
//! (vim's `:make`, `quickfix_fill.rs`), with columns in chars.
//!
//! Each session a task tab starts is a run in the workbench's
//! [`crate::runs::Runs`], of the kind the task's group says: finished from
//! its exit code, cancelled if its tab closes while it runs. A rerun in the
//! tab is a new run. The tab of each of its runs is known while it's open
//! ([`Workbench::task_view_of_run`]), for the Runs panel to show.
//!
//! A run started by [`Workbench::run_task_reading`] hands its caller how its
//! session exited and the lines of its output the caller keeps
//! ([`TaskExit`]): the debugger's pre-launch builds, which read cargo's
//! `compiler-artifact` messages (docs/decisions/r1b9dw3g0g-debugger.md §7).
//! The lines come from the pty as the program wrote them
//! ([`fe_terminal::OutputTap`]), whatever the grid wraps or its scrollback
//! drops. Its tab is an ordinary task tab, matcher and all.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
};

use fe_editor::diagnostics::{Problem, Severity};
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use fe_text::Text;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Subscription, Window};
use kiln_l10n::tr;

use crate::{
    Workbench,
    dock::problems_panel::{ColumnUnit, SourceProblem, byte_column, read_text},
    runs::{MAX_FINISHED, RunEnd, RunId, RunKind, RunSummary, Runs},
    task_defs::{Group, Task},
    task_problems::{self, Matcher, TaskProblem},
    workbench::toast::{self, Toast},
};

/// What hears the problems one run found, once its session exits
/// ([`Workbench::run_task_then`]).
pub(crate) type RunHeard =
    Box<dyn FnOnce(&mut Workbench, Vec<RunProblem>, &mut Context<Workbench>)>;

/// How one run's session exited, as a caller reading it at exit hears it
/// ([`Workbench::run_task_reading`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskExit {
    /// `None` when a signal ended it or the code is unknown.
    pub code: Option<i32>,
    /// The lines of its output the reader keeps, in order, as the program
    /// wrote them (a `\r` before the newline dropped).
    pub lines: Vec<String>,
}

/// Which lines of a run's output a reader keeps.
pub(crate) type KeepLine = fn(&str) -> bool;

/// The longest line a [`Capture`] holds while it's written: a longer one
/// is dropped whole.
const MAX_LINE: usize = 16 << 20;

/// The lines a reader keeps of a run's output, as the pty's tap hears it.
struct Capture {
    keep: KeepLine,
    /// The line being written.
    partial: Vec<u8>,
    /// The line being written outgrew [`MAX_LINE`].
    overflowed: bool,
    lines: Vec<String>,
}

impl Capture {
    fn new(keep: KeepLine) -> Self {
        Self {
            keep,
            partial: Vec::new(),
            overflowed: false,
            lines: Vec::new(),
        }
    }

    fn hear(&mut self, bytes: &[u8]) {
        for chunk in bytes.split_inclusive(|byte| *byte == b'\n') {
            let (body, ends) = match chunk.strip_suffix(b"\n") {
                Some(body) => (body, true),
                None => (chunk, false),
            };
            if self.partial.len() + body.len() > MAX_LINE {
                self.overflowed = true;
                self.partial.clear();
            } else if !self.overflowed {
                self.partial.extend_from_slice(body);
            }
            if ends {
                self.end_line();
            }
        }
    }

    fn end_line(&mut self) {
        if !self.overflowed {
            let line = String::from_utf8_lossy(&self.partial);
            let line = line.strip_suffix('\r').unwrap_or(&line);
            if (self.keep)(line) {
                self.lines.push(line.to_owned());
            }
        }
        self.partial.clear();
        self.overflowed = false;
    }

    /// The lines kept, the last one too if it had no newline.
    fn take(&mut self) -> Vec<String> {
        if !self.partial.is_empty() {
            self.end_line();
        }
        std::mem::take(&mut self.lines)
    }
}

/// A [`Capture`], shared with the pty's thread.
type Captured = Arc<Mutex<Capture>>;

/// What reads one run's exit ([`Workbench::run_task_reading`]): `None` if
/// its tab closed, or its session was replaced, before it exited.
pub(crate) type ExitHeard =
    Box<dyn FnOnce(&mut Workbench, Option<TaskExit>, &mut Context<Workbench>)>;

/// An [`ExitHeard`] until it's called: by the session's exit, or by its tab
/// going first.
type Reading = Rc<RefCell<Option<ExitHeard>>>;

/// A problem one run found, as its caller hears it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunProblem {
    pub path: PathBuf,
    /// From 0.
    pub row: u32,
    /// From 0, in chars: the tool's column as the file on disk reads, else
    /// as the tool printed it.
    pub column: u32,
    pub severity: task_problems::Severity,
    pub message: String,
}

/// The output the tests' fake sessions, which never start, leave once they
/// exit.
#[cfg(test)]
pub(crate) struct FakeTaskOutput(pub(crate) String);

#[cfg(test)]
impl gpui::Global for FakeTaskOutput {}

/// What running a task says in a folder that isn't trusted.
pub(crate) fn untrusted_message() -> String {
    tr!("workbench-lsp-tasks-untrusted").into()
}

/// The terminal panel's task tabs, each with its source in the Problems panel,
/// which keeps this: the terminal panel's tabs are its own, known here by the
/// session each one shows.
#[derive(Default)]
pub(crate) struct TaskTabs {
    tabs: Vec<TaskTab>,
    /// The key the next tab gets.
    next_key: u64,
    /// To the terminal panel, for its tabs closing.
    closing: Option<Subscription>,
}

struct TaskTab {
    /// Its source's key, kept by a rerun.
    key: u64,
    /// The view of the session it shows, whose exit counts.
    view: EntityId,
    /// To that session exiting.
    _exited: Subscription,
    /// Reading the session's output: dropped with the session it read.
    _matching: Option<gpui::Task<()>>,
    /// What hears this session's problems, if its run was asked for by
    /// [`Workbench::run_task_then`]: not a rerun's.
    heard: Option<RunHeard>,
    /// What reads this session's exit, if its run was asked for by
    /// [`Workbench::run_task_reading`].
    reading: Option<Reading>,
    /// The session's run in the workbench's [`Runs`].
    run: RunId,
    /// The runs of the sessions it showed before, oldest first: no more
    /// than [`MAX_FINISHED`], as many as the runs keep.
    earlier: Vec<RunId>,
}

/// The kind of run `task` is: its group's, a script unless it builds or
/// tests.
fn run_kind(task: &Task) -> RunKind {
    match task.group {
        Some(Group::Build) => RunKind::Build,
        Some(Group::Test) => RunKind::Test,
        Some(Group::Run) | None => RunKind::Script,
    }
}

/// How a session that exited with `code` ended, and what its run says.
fn run_end(code: Option<i32>) -> (RunEnd, Option<RunSummary>) {
    match code {
        Some(0) => (RunEnd::Passed, Some(RunSummary::Exit(0))),
        Some(code) => (
            RunEnd::Failed { code: Some(code) },
            Some(RunSummary::Exit(code)),
        ),
        None => (RunEnd::Failed { code: None }, None),
    }
}

/// How to read one run's output.
struct OutputReader {
    /// The task's label, its problems' source.
    label: String,
    matcher: Option<Matcher>,
    cwd: PathBuf,
    root: PathBuf,
}

/// The matcher `task` names, else its source's default. One the file misnames
/// reads nothing.
fn task_matcher(task: &Task) -> Option<Matcher> {
    let Some(name) = &task.problem_matcher else {
        return Matcher::default_for(task.source);
    };
    Matcher::from_name(name)
        .inspect_err(|error| log::warn!("task {}: {error}", task.label))
        .ok()
}

/// What the columns in `output` count, as each tool prints them.
fn column_unit(matcher: Matcher, output: &str) -> ColumnUnit {
    match matcher {
        Matcher::Rustc => ColumnUnit::Chars,
        Matcher::Tsc | Matcher::Eslint => ColumnUnit::Utf16,
        Matcher::Go | Matcher::Python => ColumnUnit::Bytes,
        Matcher::Gcc if from_clang(output) => ColumnUnit::Bytes,
        Matcher::Gcc => ColumnUnit::Display,
    }
}

/// Whether `output` has the line clang ends a file's diagnostics with, as
/// `2 errors generated.`, which gcc doesn't print: they share a format, but
/// not what a column counts. All a line of either says is its position, so
/// clang's with no such line (`-fno-caret-diagnostics`) reads as gcc's, and a
/// gcc's beside clang's as clang's: the same row, the wrong column on one with
/// tabs or wide text.
fn from_clang(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim_end();
        line.ends_with(" generated.") && line.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// `problem`, as a [`RunProblem`]: its column in chars in `text`, the file
/// it names.
fn run_problem(problem: &SourceProblem, text: Option<&Text>) -> RunProblem {
    let row = problem.problem.row;
    // The panel's `file_column` is in bytes when the file was read.
    let column = match text {
        Some(text) if row < text.row_count() => {
            let line = text.row_to_string(row);
            let bytes = problem.file_column as usize;
            (line.char_indices().take_while(|(ix, _)| *ix < bytes)).count() as u32
        }
        _ => problem.problem.column,
    };
    let severity = match problem.problem.severity {
        Severity::Error => task_problems::Severity::Error,
        _ => task_problems::Severity::Warning,
    };
    RunProblem {
        path: problem.path.clone(),
        row,
        column,
        severity,
        message: problem.problem.message.clone(),
    }
}

/// `problem`, from the task `label`, as the panel lists it. It has no range:
/// its offsets aren't known until its file's opened.
fn source_problem(
    problem: TaskProblem,
    label: &str,
    unit: ColumnUnit,
    text: Option<&Text>,
) -> SourceProblem {
    let (row, column) = (
        problem.line.saturating_sub(1),
        problem.column.saturating_sub(1),
    );
    let severity = match problem.severity {
        task_problems::Severity::Error => Severity::Error,
        task_problems::Severity::Warning => Severity::Warning,
    };
    SourceProblem {
        path: problem.path,
        problem: Problem {
            range: 0..0,
            row,
            column,
            severity,
            message: problem.message,
            source: Some(label.to_owned()),
            code: None,
            related: Vec::new(),
            code_href: None,
        },
        unit,
        file_column: text.map_or(column, |text| byte_column(text, row, column, unit)),
    }
}

/// Tells `reading`, if it's still waiting, that its session went before it
/// exited.
fn hear_gone(reading: Option<Reading>, workbench: &mut Workbench, cx: &mut Context<Workbench>) {
    let heard = reading.and_then(|reading| reading.borrow_mut().take());
    if let Some(heard) = heard {
        heard(workbench, None, cx);
    }
}

/// The lines `captured` kept of `view`'s session (a test's fake session,
/// which never starts, leaves [`FakeTaskOutput`]'s, if set).
fn session_lines(view: &Entity<TerminalView>, captured: &Captured, cx: &App) -> Vec<String> {
    let mut capture = captured.lock().unwrap_or_else(PoisonError::into_inner);
    #[cfg(test)]
    if view.read(cx).terminal().is_none() && cx.has_global::<FakeTaskOutput>() {
        capture.hear(cx.global::<FakeTaskOutput>().0.as_bytes());
    }
    #[cfg(not(test))]
    let _ = (view, cx);
    capture.take()
}

impl Workbench {
    /// The builds, tests and scripts run here, newest first, for a panel to
    /// list and observe.
    pub fn runs(&self) -> &Entity<Runs> {
        &self.runs
    }

    /// Runs `task` and shows the terminal dock with it, focused there as a
    /// new terminal is, and remembers it as the last task. A task's command
    /// comes from the project, so one in a root that isn't trusted is refused
    /// with a toast offering to trust it.
    pub fn run_task(&mut self, task: Task, window: &mut Window, cx: &mut Context<Self>) {
        self.run_task_then(task, None, window, cx);
    }

    /// [`Workbench::run_task`], then once its session exits, hands what its
    /// matcher found, as the Problems panel lists them, to `heard`: not if
    /// it doesn't start, nor if its tab closes or reruns before then.
    pub(crate) fn run_task_then(
        &mut self,
        task: Task,
        heard: Option<RunHeard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_task_with(task, heard, None, window, cx);
    }

    /// [`Workbench::run_task`], then once its session exits, hands `reading`
    /// its exit code and the lines of all its output that `keep` keeps; or
    /// `None` if its tab closes or reruns first. Nothing if it doesn't
    /// start: whether it started.
    pub(crate) fn run_task_reading(
        &mut self,
        task: Task,
        keep: KeepLine,
        reading: ExitHeard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.run_task_with(task, None, Some((keep, reading)), window, cx)
    }

    fn run_task_with(
        &mut self,
        task: Task,
        heard: Option<RunHeard>,
        reading: Option<(KeepLine, ExitHeard)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.root_trusted() {
            let trust = fe_command::CommandId::parse("workbench.trust_folder")
                .expect("the command's id is valid");
            let toast = Toast::warning(untrusted_message())
                .action(tr!("workbench-lsp-tasks-trust-folder"), trust);
            toast::show(cx, toast);
            return false;
        }
        let terminal = self.terminal().clone();
        let before: Vec<EntityId> = (terminal.read(cx).views()).map(Entity::entity_id).collect();
        let started = terminal.update(cx, |terminal, cx| terminal.add_task(&task, window, cx));
        if started {
            // Before its first layout, when its session starts.
            let reading = reading.map(|(keep, reading)| {
                let captured: Captured = Arc::new(Mutex::new(Capture::new(keep)));
                let tap = captured.clone();
                if let Some(view) = terminal.read(cx).view().cloned() {
                    view.update(cx, |view, _| {
                        view.tap_output(Arc::new(move |bytes| {
                            let mut capture = tap.lock().unwrap_or_else(PoisonError::into_inner);
                            capture.hear(bytes);
                        }));
                    });
                }
                (captured, reading)
            });
            self.follow_task_tab(&task, &before, heard, reading, cx);
            self.show_terminal_dock(window, cx);
        }
        started
    }

    /// Hears the session `task` just started in the active tab. If it took the
    /// place of one of the sessions `before`, its tab keeps its source, and
    /// what that run found stays listed until this one's done.
    fn follow_task_tab(
        &mut self,
        task: &Task,
        before: &[EntityId],
        heard: Option<RunHeard>,
        reading: Option<(Captured, ExitHeard)>,
        cx: &mut Context<Self>,
    ) {
        let terminal = self.terminal().clone();
        let Some(view) = terminal.read(cx).view().cloned() else {
            return;
        };
        let replaced = (before.iter().copied()).find(|id| {
            let mut views = terminal.read(cx).views();
            !views.any(|view| view.entity_id() == *id)
        });
        let reader = OutputReader {
            label: task.label.clone(),
            matcher: task_matcher(task),
            cwd: task.cwd.clone(),
            root: (self.root()).map_or_else(|| task.cwd.clone(), ToOwned::to_owned),
        };
        let runs = self.runs.clone();
        let run = runs.update(cx, |runs, cx| runs.begin(run_kind(task), &task.label, cx));
        let (captured, reading) = match reading {
            Some((captured, reading)) => (Some(captured), Some(reading)),
            None => (None, None),
        };
        let reading: Option<Reading> = reading.map(|reading| Rc::new(RefCell::new(Some(reading))));
        let exited = cx.subscribe(&view, {
            let reading = reading.clone();
            move |workbench, view, event: &TerminalViewEvent, cx| {
                if let TerminalViewEvent::Exited(code) = *event {
                    let (end, summary) = run_end(code);
                    (workbench.runs.clone())
                        .update(cx, |runs, cx| runs.finish(run, end, summary, cx));
                    workbench.read_task_output(&view, &reader, cx);
                    let heard = reading
                        .as_ref()
                        .and_then(|reading| reading.borrow_mut().take());
                    if let Some((heard, captured)) = heard.zip(captured.as_ref()) {
                        let lines = session_lines(&view, captured, cx);
                        heard(workbench, Some(TaskExit { code, lines }), cx);
                    }
                }
            }
        });
        let closing = cx.observe(&terminal, |workbench, _, cx| {
            workbench.forget_closed_task_tabs(cx);
        });
        let view = view.entity_id();
        let panel = self.dock().read(cx).problems().clone();
        let (replaced_run, unheard) = panel.update(cx, |panel, _| {
            let tabs = &mut panel.task_tabs;
            tabs.closing.get_or_insert(closing);
            let tab = (tabs.tabs.iter_mut()).find(|tab| Some(tab.view) == replaced);
            match tab {
                Some(tab) => {
                    tab.view = view;
                    tab._exited = exited;
                    tab._matching = None;
                    tab.heard = heard;
                    let unheard = std::mem::replace(&mut tab.reading, reading);
                    let replaced = std::mem::replace(&mut tab.run, run);
                    tab.earlier.push(replaced);
                    if tab.earlier.len() > MAX_FINISHED {
                        tab.earlier.remove(0);
                    }
                    (Some(replaced), unheard)
                }
                None => {
                    let key = tabs.next_key;
                    tabs.next_key += 1;
                    tabs.tabs.push(TaskTab {
                        key,
                        view,
                        _exited: exited,
                        _matching: None,
                        heard,
                        reading,
                        run,
                        earlier: Vec::new(),
                    });
                    (None, None)
                }
            }
        });
        // The session it replaced never exits now.
        hear_gone(unheard, self, cx);
        // The tab reruns once its session's done, so this is only for one
        // whose end went unheard.
        if let Some(replaced) = replaced_run {
            runs.update(cx, |runs, cx| {
                runs.finish(replaced, RunEnd::Cancelled, None, cx)
            });
        }
    }

    /// Reads the output of the session `view` shows, which has exited, off the
    /// main thread. A session that never started has none, and its tab closes.
    fn read_task_output(
        &mut self,
        view: &Entity<TerminalView>,
        reader: &OutputReader,
        cx: &mut Context<Self>,
    ) {
        let output = match view.read(cx).terminal() {
            Some(terminal) => reader.matcher.map(|_| terminal.output_text()),
            #[cfg(test)]
            None if cx.has_global::<FakeTaskOutput>() => {
                let FakeTaskOutput(text) = cx.global::<FakeTaskOutput>();
                reader.matcher.map(|_| text.clone())
            }
            None => return,
        };
        let id = view.entity_id();
        let panel = self.dock().read(cx).problems().clone();
        let hearing =
            (panel.read(cx).task_tabs.tabs.iter()).any(|tab| tab.view == id && tab.heard.is_some());
        let matched = reader.matcher.zip(output).map(|(matcher, text)| {
            let (cwd, root) = (reader.cwd.clone(), reader.root.clone());
            let label = reader.label.clone();
            cx.background_spawn(async move {
                let exists = |path: &std::path::Path| path.is_file();
                let result = task_problems::match_output(matcher, &text, &cwd, &root, &exists);
                if result.truncated {
                    log::warn!(
                        "task {label}: listing its first {} problems",
                        task_problems::MAX_PROBLEMS
                    );
                }
                let unit = column_unit(matcher, &text);
                let mut files: HashMap<PathBuf, Option<Text>> = HashMap::new();
                let mut heard = Vec::new();
                let problems = (result.problems.into_iter())
                    .map(|problem| {
                        let file = (files.entry(problem.path.clone()))
                            .or_insert_with_key(|path| read_text(path.clone()));
                        let problem = source_problem(problem, &label, unit, file.as_ref());
                        if hearing {
                            heard.push(run_problem(&problem, file.as_ref()));
                        }
                        problem
                    })
                    .collect::<Vec<_>>();
                (problems, heard)
            })
        });
        let matching = cx.spawn(async move |workbench, cx| {
            let (problems, heard) = match matched {
                Some(matching) => matching.await,
                None => Default::default(),
            };
            let listed = workbench.update(cx, |workbench, cx| {
                workbench.task_output_read(id, problems, heard, cx)
            });
            listed.ok();
        });
        panel.update(cx, |panel, _| {
            let tabs = &mut panel.task_tabs.tabs;
            if let Some(tab) = tabs.iter_mut().find(|tab| tab.view == id) {
                tab._matching = Some(matching);
            }
        });
    }

    /// Lists what the run of the session `view` found in place of what its tab
    /// last found, if the tab still shows it, then hands `heard`, the same
    /// problems, to what asked to hear them.
    fn task_output_read(
        &mut self,
        view: EntityId,
        problems: Vec<SourceProblem>,
        heard: Vec<RunProblem>,
        cx: &mut Context<Self>,
    ) {
        let panel = self.dock().read(cx).problems().clone();
        let found = panel.update(cx, |panel, _| {
            let tab = panel.task_tabs.tabs.iter_mut().find(|tab| tab.view == view);
            let tab = tab.map(|tab| (tab.key, tab.heard.take()));
            if let Some((key, _)) = tab {
                panel.set_source(key, problems);
            }
            tab
        });
        if let Some((_, hearing)) = found {
            self.sync_problems(cx);
            if let Some(hearing) = hearing {
                hearing(self, heard, cx);
            }
        }
    }

    /// Drops the problems of the task tabs the terminal panel no longer has.
    fn forget_closed_task_tabs(&mut self, cx: &mut Context<Self>) {
        let open: Vec<EntityId> = (self.terminal().read(cx).views())
            .map(Entity::entity_id)
            .collect();
        let panel = self.dock().read(cx).problems().clone();
        let (closed, unheard) = panel.update(cx, |panel, _| {
            let (mut closed, kept) = std::mem::take(&mut panel.task_tabs.tabs)
                .into_iter()
                .partition::<Vec<_>, _>(|tab| !open.contains(&tab.view));
            panel.task_tabs.tabs = kept;
            for tab in &closed {
                panel.clear_source(tab.key);
            }
            let unheard: Vec<Reading> = (closed.iter_mut())
                .filter_map(|tab| tab.reading.take())
                .collect();
            (
                closed.iter().map(|tab| tab.run).collect::<Vec<_>>(),
                unheard,
            )
        });
        // Their sessions never exit now.
        for reading in unheard {
            hear_gone(Some(reading), self, cx);
        }
        // Those still running are cancelled: closing a tab ends its session.
        (self.runs.clone()).update(cx, |runs, cx| {
            for &run in &closed {
                runs.finish(run, RunEnd::Cancelled, None, cx);
            }
        });
        if !closed.is_empty() {
            self.sync_problems(cx);
        }
    }

    /// The view of the task tab whose session made `run`, or made one of
    /// its earlier runs, while the tab's open.
    pub(crate) fn task_view_of_run(&self, run: RunId, cx: &App) -> Option<EntityId> {
        let panel = self.dock().read(cx).problems().read(cx);
        (panel.task_tabs.tabs.iter())
            .find(|tab| tab.run == run || tab.earlier.contains(&run))
            .map(|tab| tab.view)
    }

    /// The task last run in this root, for a rerun.
    pub fn last_task(&self, cx: &App) -> Option<Task> {
        self.terminal().read(cx).last_task().cloned()
    }
}
