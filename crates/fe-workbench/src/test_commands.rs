//! The test commands, VS Code's Run Test at Cursor and neotest's run nearest:
//!
//! - `tests.run_at_cursor`: the innermost test holding the active editor's
//!   caret, else the test module or class around it
//!   ([`crate::test_runnables`]).
//! - `tests.run_file`: the active file's tests.
//! - `tests.rerun_last`: the test last run through these, in this window's
//!   root, apart from `tasks.rerun_last`'s task.
//!
//! Each saves the window's files first, as VS Code's tasks do: a test runs
//! what's on disk, and a file that won't save stops it. Then it builds a
//! task, reading the package it's in off the main thread, and runs it with [`Workbench::run_task`]: in a terminal tab, its failures in
//! the Problems panel, and only in a trusted root.
//!
//! `Workbench`'s one `#[kiln::command]` block is `workbench_commands.rs`'s,
//! so the commands are declared on [`TestCommands`], which no element
//! attaches, and [`init`]'s listeners run them on the window's workbench, as
//! `save_commands::init`'s do.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui::{App, AppContext as _, Context, EntityId, Global, Window};
use kiln::{Cx, platform::command_actions::dispatch_window};
use kiln_l10n::tr;

use crate::{
    Workbench,
    status_bar::{language_label, plain_text},
    task_commands::project_files,
    task_defs::Task,
    test_runnables::{Runnable, TEST_LANGUAGES, no_runner, runnable_at, runnables, test_command},
    workbench::toast::{self, Toast},
};

/// What `tests.run_at_cursor` says with no test around the caret.
pub(crate) fn no_test_here() -> String {
    tr!("workbench-lsp-tests-none-here").into()
}

/// What `tests.rerun_last` says before any test has run.
pub(crate) fn no_test_run() -> String {
    tr!("workbench-lsp-tests-none-run").into()
}

/// What the commands say with no file to test: none active, or one never
/// saved.
fn no_file() -> String {
    tr!("workbench-lsp-tests-no-file").into()
}

/// What the commands say with no folder open, whose trust a run needs.
fn no_folder() -> String {
    tr!("workbench-lsp-tests-no-folder").into()
}

/// What the commands say when a file couldn't be saved: a test runs what's
/// on disk.
fn not_saved() -> String {
    tr!("workbench-lsp-tests-not-saved").into()
}

/// What `tests.run_at_cursor` says while the file's language loads.
fn not_ready() -> String {
    tr!("workbench-lsp-tests-not-ready").into()
}

/// The commands' declaration: see the module's docs. They're handled by
/// [`init`]'s listeners, not here.
#[allow(dead_code)]
struct TestCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl TestCommands {
    /// Run Test at Cursor
    #[command(id = "tests.run_at_cursor")]
    fn run_test_at_cursor(&mut self, _: &mut Cx<Self>) {}

    /// Run Test at Row: the lens above a test, which runs that test, as
    /// `tests.run_at_cursor` does for the caret, without moving it.
    #[command(id = "tests.run_row", palette = false)]
    fn run_test_at_row(&mut self, _: &mut Cx<Self>) {}

    /// Run Tests in File
    #[command(id = "tests.run_file")]
    fn run_tests_in_file(&mut self, _: &mut Cx<Self>) {}

    /// Rerun Last Test
    #[command(id = "tests.rerun_last")]
    fn rerun_last_test(&mut self, _: &mut Cx<Self>) {}
}

/// The test each workbench last ran, and the root it ran in: a new root's
/// tests are its own.
#[derive(Default)]
struct LastTests(HashMap<EntityId, (PathBuf, Task)>);

impl Global for LastTests {}

/// What a run is of.
#[derive(Clone, Copy)]
enum Target {
    /// The test at the caret.
    Cursor,
    /// The test at a row, as a lens gave it.
    Row(u32),
    /// The whole file.
    File,
}

/// What the active editor has to run tests from.
struct TestFile {
    path: PathBuf,
    language: &'static str,
    runnable: Runnable,
}

impl Workbench {
    /// `tests.run_at_cursor`, `tests.run_row` and `tests.run_file` (`target`): saves the
    /// window's files, as a test runs what's on disk, then builds the test's
    /// task off the main thread, and runs it if the root is still the one it
    /// was built for.
    fn run_test(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root().map(Path::to_owned) else {
            return toast::show(cx, Toast::info(no_folder()));
        };
        let TestFile {
            path,
            language,
            runnable,
        } = match self.test_file(target, cx) {
            Ok(found) => found,
            Err(message) => return toast::show(cx, Toast::info(message)),
        };
        let saving = self.save_all(window, cx);
        let files = project_files(cx);
        cx.spawn_in(window, async move |workbench, cx| {
            saving.await;
            let saved = workbench.update(cx, |workbench, cx| {
                workbench.root() == Some(root.as_path()) && workbench.all_saved(cx)
            });
            if !saved.unwrap_or(false) {
                return;
            }
            let building = cx.background_spawn({
                let root = root.clone();
                async move { test_command(&*files, language, &runnable, &path, &root) }
            });
            let built = building.await;
            let ran = workbench.update_in(cx, |workbench, window, cx| {
                if workbench.root() != Some(root.as_path()) {
                    return;
                }
                match built {
                    Ok(task) => workbench.run_test_task(task, window, cx),
                    Err(message) => toast::show(cx, Toast::info(message)),
                }
            });
            ran.ok();
        })
        .detach();
    }

    /// Whether every file the window edits is saved; if not, says the tests
    /// won't run. A save that failed has said why.
    fn all_saved(&self, cx: &mut Context<Self>) -> bool {
        let unsaved = self.file_editors(cx).into_iter().any(|editor| {
            let view = editor.read(cx);
            let model = view.model();
            model.path().is_some() && !view.is_read_only() && model.is_dirty()
        });
        if unsaved {
            toast::show(cx, Toast::warning(not_saved()));
        }
        !unsaved
    }

    /// The active editor's file and what in it to run: the test at the caret
    /// or the given row, else the whole file. Why there's none, in words.
    fn test_file(&self, target: Target, cx: &mut Context<Self>) -> Result<TestFile, String> {
        let editor = self.active_editor(cx).ok_or_else(no_file)?.clone();
        // The caret and the tree are both read from what this catches up.
        editor.update(cx, |view, _| view.model_mut().sync_display());
        let view = editor.read(cx);
        let path = view.model().path().ok_or_else(no_file)?.to_owned();
        if view.is_loading_language() {
            return Err(not_ready());
        }
        let syntax = view.syntax();
        let language = syntax.map(|syntax| syntax.language().name());
        let Some(language) = language.filter(|language| TEST_LANGUAGES.contains(language)) else {
            let label = language.map_or_else(plain_text, language_label);
            return Err(no_runner(&label));
        };
        let snapshot = view.model().snapshot();
        let text = snapshot.text();
        if matches!(target, Target::File) {
            let runnable = Runnable::file(&path, text.row_count());
            return Ok(TestFile {
                path,
                language,
                runnable,
            });
        }
        let syntax = syntax.filter(|syntax| {
            syntax.version() == snapshot.version() && !syntax.needs_parse() && !syntax.failed()
        });
        let Some(syntax) = syntax else {
            return Err(not_ready());
        };
        let row = match target {
            Target::Row(row) => row,
            _ => (text.offset_to_point(view.model().primary_selection().head())).row,
        };
        let found = runnables(language, syntax, text);
        let runnable = runnable_at(&found, row).ok_or_else(no_test_here)?.clone();
        Ok(TestFile {
            path,
            language,
            runnable,
        })
    }

    /// Runs a test's `task`, remembered for `tests.rerun_last` if the root
    /// lets it run.
    fn run_test_task(&mut self, task: Task, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(root) = self.root().filter(|_| self.root_trusted()) {
            let entry = (root.to_owned(), task.clone());
            let id = cx.entity_id();
            cx.default_global::<LastTests>().0.insert(id, entry);
        }
        self.run_task(task, window, cx);
    }

    /// `tests.rerun_last`: saves the window's files and runs the last test
    /// again, as it was then.
    fn rerun_last_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.last_test(cx) else {
            return toast::show(cx, Toast::info(no_test_run()));
        };
        let root = self.root().map(Path::to_owned);
        let saving = self.save_all(window, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            saving.await;
            let ran = workbench.update_in(cx, |workbench, window, cx| {
                if workbench.root() == root.as_deref() && workbench.all_saved(cx) {
                    workbench.run_test_task(task, window, cx);
                }
            });
            ran.ok();
        })
        .detach();
    }

    /// The test last run in this root, for a rerun.
    pub(crate) fn last_test(&self, cx: &Context<Self>) -> Option<Task> {
        let (root, task) = cx.try_global::<LastTests>()?.0.get(&cx.entity_id())?;
        (self.root() == Some(root.as_path())).then(|| task.clone())
    }
}

/// Runs `run` on the workbench of the window the command was dispatched in,
/// after the dispatch, which has the window out.
fn on_workbench(
    cx: &mut App,
    run: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
) {
    let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
        return;
    };
    cx.defer(move |cx| {
        handle
            .update(cx, |_, window, cx| {
                if let Some(Some(workbench)) = window.root::<Workbench>() {
                    workbench.update(cx, |workbench, cx| run(workbench, window, cx));
                }
            })
            .ok();
    });
}

/// Handles the commands.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &RunTestAtCursor, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.run_test(Target::Cursor, window, cx)
        });
    });
    cx.on_action(|_: &RunTestAtRow, cx| {
        // The row is the click's argument, gone once the dispatch ends.
        let row = (kiln::current_invocation(cx))
            .and_then(|invocation| invocation.args.get("row")?.as_u64())
            .and_then(|row| u32::try_from(row).ok());
        if let Some(row) = row {
            on_workbench(cx, move |workbench, window, cx| {
                workbench.run_test(Target::Row(row), window, cx)
            });
        }
    });
    cx.on_action(|_: &RunTestsInFile, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.run_test(Target::File, window, cx)
        });
    });
    cx.on_action(|_: &RerunLastTest, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.rerun_last_test(window, cx)
        });
    });
}
