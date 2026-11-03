//! Running a project's task ([`crate::task_defs`]): in a tab of its own on the
//! terminal panel, whose output, rerun and exit report are the panel's
//! (`TerminalPanel::add_task`). The commands that get here are
//! [`crate::task_commands`]'s.
//!
//! As a task's session exits, its output goes through its problem matcher
//! ([`crate::task_problems`]) and the files it names are read, off the main
//! thread, and the Problems panel
//! lists what it found as the tab's source: replacing what its last run found,
//! until the tab closes. Two tabs of one label are two sources.

use std::{collections::HashMap, path::PathBuf};

use fe_editor::diagnostics::{Problem, Severity};
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use fe_text::Text;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Subscription, Window};

use crate::{
    Workbench,
    dock::problems_panel::{ColumnUnit, SourceProblem, byte_column, read_text},
    task_defs::Task,
    task_problems::{self, Matcher, TaskProblem},
    workbench::toast::{self, Toast},
};

/// What running a task says in a folder that isn't trusted.
pub(crate) const UNTRUSTED: &str = "Trust this folder to run its tasks";

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
            related: Vec::new(),
            code_href: None,
        },
        unit,
        file_column: text.map_or(column, |text| byte_column(text, row, column, unit)),
    }
}

impl Workbench {
    /// Runs `task` and shows the terminal dock with it, focused there as a
    /// new terminal is, and remembers it as the last task. A task's command
    /// comes from the project, so one in a root that isn't trusted is refused
    /// with a toast offering to trust it.
    pub fn run_task(&mut self, task: Task, window: &mut Window, cx: &mut Context<Self>) {
        if !self.root_trusted() {
            let trust = fe_command::CommandId::parse("workbench.trust_folder")
                .expect("the command's id is valid");
            let toast = Toast::warning(UNTRUSTED).action("Trust Folder", trust);
            return toast::show(cx, toast);
        }
        let terminal = self.terminal().clone();
        let before: Vec<EntityId> = (terminal.read(cx).views()).map(Entity::entity_id).collect();
        let started = terminal.update(cx, |terminal, cx| terminal.add_task(&task, window, cx));
        if started {
            self.follow_task_tab(&task, &before, cx);
            self.show_terminal_dock(window, cx);
        }
    }

    /// Hears the session `task` just started in the active tab. If it took the
    /// place of one of the sessions `before`, its tab keeps its source, and
    /// what that run found stays listed until this one's done.
    fn follow_task_tab(&mut self, task: &Task, before: &[EntityId], cx: &mut Context<Self>) {
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
        let exited = cx.subscribe(
            &view,
            move |workbench, view, TerminalViewEvent::Exited(_), cx| {
                workbench.read_task_output(&view, &reader, cx);
            },
        );
        let closing = cx.observe(&terminal, |workbench, _, cx| {
            workbench.forget_closed_task_tabs(cx);
        });
        let view = view.entity_id();
        let panel = self.dock().read(cx).problems().clone();
        panel.update(cx, |panel, _| {
            let tabs = &mut panel.task_tabs;
            tabs.closing.get_or_insert(closing);
            let tab = (tabs.tabs.iter_mut()).find(|tab| Some(tab.view) == replaced);
            match tab {
                Some(tab) => {
                    tab.view = view;
                    tab._exited = exited;
                    tab._matching = None;
                }
                None => {
                    let key = tabs.next_key;
                    tabs.next_key += 1;
                    tabs.tabs.push(TaskTab {
                        key,
                        view,
                        _exited: exited,
                        _matching: None,
                    });
                }
            }
        });
    }

    /// Reads the output of the session `view` shows, which has exited, off the
    /// main thread. A session that never started has none, and its tab closes.
    fn read_task_output(
        &mut self,
        view: &Entity<TerminalView>,
        reader: &OutputReader,
        cx: &mut Context<Self>,
    ) {
        let Some(terminal) = view.read(cx).terminal() else {
            return;
        };
        let text = reader.matcher.map(|_| terminal.output_text());
        let id = view.entity_id();
        let matched = reader.matcher.zip(text).map(|(matcher, text)| {
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
                (result.problems.into_iter())
                    .map(|problem| {
                        let file = (files.entry(problem.path.clone()))
                            .or_insert_with_key(|path| read_text(path.clone()));
                        source_problem(problem, &label, unit, file.as_ref())
                    })
                    .collect::<Vec<_>>()
            })
        });
        let matching = cx.spawn(async move |workbench, cx| {
            let problems = match matched {
                Some(matching) => matching.await,
                None => Vec::new(),
            };
            let listed = workbench.update(cx, |workbench, cx| {
                workbench.task_output_read(id, problems, cx)
            });
            listed.ok();
        });
        let panel = self.dock().read(cx).problems().clone();
        panel.update(cx, |panel, _| {
            let tabs = &mut panel.task_tabs.tabs;
            if let Some(tab) = tabs.iter_mut().find(|tab| tab.view == id) {
                tab._matching = Some(matching);
            }
        });
    }

    /// Lists what the run of the session `view` found in place of what its tab
    /// last found, if the tab still shows it.
    fn task_output_read(
        &mut self,
        view: EntityId,
        problems: Vec<SourceProblem>,
        cx: &mut Context<Self>,
    ) {
        let panel = self.dock().read(cx).problems().clone();
        let found = panel.update(cx, |panel, _| {
            let tab = panel.task_tabs.tabs.iter().find(|tab| tab.view == view);
            let key = tab.map(|tab| tab.key);
            if let Some(key) = key {
                panel.set_source(key, problems);
            }
            key.is_some()
        });
        if found {
            self.sync_problems(cx);
        }
    }

    /// Drops the problems of the task tabs the terminal panel no longer has.
    fn forget_closed_task_tabs(&mut self, cx: &mut Context<Self>) {
        let open: Vec<EntityId> = (self.terminal().read(cx).views())
            .map(Entity::entity_id)
            .collect();
        let panel = self.dock().read(cx).problems().clone();
        let closed = panel.update(cx, |panel, _| {
            let (closed, kept) = std::mem::take(&mut panel.task_tabs.tabs)
                .into_iter()
                .partition::<Vec<_>, _>(|tab| !open.contains(&tab.view));
            panel.task_tabs.tabs = kept;
            for tab in &closed {
                panel.clear_source(tab.key);
            }
            !closed.is_empty()
        });
        if closed {
            self.sync_problems(cx);
        }
    }

    /// The task last run in this root, for a rerun.
    pub fn last_task(&self, cx: &App) -> Option<Task> {
        self.terminal().read(cx).last_task().cloned()
    }
}
