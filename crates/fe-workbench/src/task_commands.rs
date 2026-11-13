//! The task commands, typed in `workbench_commands.rs`:
//!
//! - `tasks.run`: a [`Picker`] over the root's tasks ([`crate::task_defs`]),
//!   each with its source and command, file tasks first, and below them what
//!   was wrong with the tasks file.
//! - `tasks.rerun_last`: the task last run in this root again.
//! - `tasks.run_build`: the default build task, or else the picker with only
//!   the build group.
//!
//! - A typed query on a root with a detected Gradle or Maven task (a build file
//!   is there) adds a final row, "Run `./gradlew <query>`" (`./mvnw`, `gradle`
//!   or `mvn`, the program the detected tasks use, Gradle's when both are
//!   there): Enter runs the program with the query split into arguments,
//!   words by whitespace and quotes grouping one ([`split_args`]), as an argv
//!   with no shell. The query goes through verbatim, so a module's task is
//!   `:sub:test` and `:app:bootRun --args=x` works. An empty or unterminated
//!   query shows the row disabled, with why. `tasks.rerun_last` repeats it.
//!
//! The list is read afresh for each command, off the main thread, so an edit
//! to the tasks file shows at the next one. Running goes through
//! [`Workbench::run_task`], which holds the root's trust: an untrusted root
//! can list its tasks but not run them.

use kiln_shell::picker::joined_label;
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_ui::{ActiveTheme as _, HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div,
};
use kiln_l10n::{text, tr};
use kiln_shell::ModalId;

use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
    task_defs::{self, Group, ProjectFiles, Source, TASKS_FILE, Task, TaskCommand, TaskDiagnostic},
    toolbar_picker::subsequence,
    workbench::toast::{self, Toast},
};

/// What `tasks.rerun_last` says before any task has run.
pub(crate) fn no_task_run() -> SharedString {
    tr!("workbench-task-none-run")
}

/// The picker's name in the modal layer.
const TASK_PICKER: ModalId = ModalId("tasks");

/// What reads a project's files, on a background thread.
pub(crate) type Files = Arc<dyn ProjectFiles + Send + Sync>;

/// A root's tasks and what was wrong with its tasks file.
pub(crate) type Listing = (Vec<Task>, Vec<TaskDiagnostic>);

#[cfg(not(test))]
pub(crate) fn project_files(_: &App) -> Files {
    Arc::new(task_defs::DiskFiles)
}

/// What the tests read a project through, [`task_defs::DiskFiles`] being out
/// of their reach.
#[cfg(test)]
pub(crate) struct TestFiles(pub(crate) Files);

#[cfg(test)]
impl gpui::Global for TestFiles {}

#[cfg(test)]
pub(crate) fn project_files(cx: &App) -> Files {
    cx.global::<TestFiles>().0.clone()
}

/// Reads `root`'s tasks, off the main thread: none with no root.
pub(crate) fn read_tasks(root: Option<PathBuf>, cx: &App) -> gpui::Task<Listing> {
    let files = project_files(cx);
    cx.background_spawn(async move {
        match root {
            Some(root) => task_defs::load(&*files, &root),
            None => Listing::default(),
        }
    })
}

/// A source's name in a task's row.
fn source_name(source: Source) -> &'static str {
    match source {
        Source::File => "tasks.toml",
        Source::Cargo => "cargo",
        Source::Npm => "npm",
        Source::Gradle => "gradle",
        Source::Maven => "maven",
        Source::Make => "make",
        Source::Just => "just",
    }
}

/// `text` on one line, a row being one line high: a shell command or a TOML
/// parse error can run over several.
fn one_line(text: &str) -> String {
    let lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    lines.collect::<Vec<_>>().join(" ")
}

/// A command as a row shows it: an argument with a space in it is quoted, so
/// it doesn't read as two.
fn command_line(command: &TaskCommand) -> String {
    match command {
        TaskCommand::Shell(line) => one_line(line),
        TaskCommand::Argv(argv) => {
            let word = |word: &String| {
                if word.is_empty() || word.contains(char::is_whitespace) {
                    format!("{word:?}")
                } else {
                    word.clone()
                }
            };
            argv.iter().map(word).collect::<Vec<_>>().join(" ")
        }
    }
}

/// What a task's row shows after its label.
fn detail(task: &Task) -> String {
    let command = command_line(&task.command);
    format!("{} · {command}", source_name(task.source))
}

/// What the list shows with nothing to run.
pub(crate) fn nothing_to_run(group: Option<Group>) -> String {
    match group {
        None => tr!("workbench-task-none", file = TASKS_FILE),
        Some(group) => tr!(
            "workbench-task-none-in-group",
            group = group.to_string(),
            file = TASKS_FILE
        ),
    }
    .to_string()
}

/// The default build task among `tasks`, the one `tasks.run_build` runs.
pub(crate) fn default_build(tasks: &[Task]) -> Option<&Task> {
    (tasks.iter()).find(|task| task.default && task.group == Some(Group::Build))
}

/// Why a typed query isn't arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SplitError {
    /// There's no word in it.
    Empty,
    /// A quote opens and doesn't close.
    Unterminated,
}

/// `query` as arguments: split at whitespace, a word's part in double or
/// single quotes (which don't nest, and have no escapes) being kept as it is,
/// whitespace included, so `--args="a b"` is one argument, `--args=a b`.
pub(crate) fn split_args(query: &str) -> Result<Vec<String>, SplitError> {
    let mut args = Vec::new();
    let mut word: Option<String> = None;
    let mut quote: Option<char> = None;
    for c in query.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => word.get_or_insert_default().push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                word.get_or_insert_default();
            }
            None if c.is_whitespace() => args.extend(word.take()),
            None => word.get_or_insert_default().push(c),
        }
    }
    if quote.is_some() {
        return Err(SplitError::Unterminated);
    }
    args.extend(word);
    if args.is_empty() {
        return Err(SplitError::Empty);
    }
    Ok(args)
}

/// The program that runs a typed query, from the detected tasks: Gradle's
/// first, else Maven's.
fn custom_runner(tasks: &[Task]) -> Option<(Source, String)> {
    [Source::Gradle, Source::Maven]
        .into_iter()
        .find_map(|source| {
            let task = tasks.iter().find(|task| task.source == source)?;
            match &task.command {
                TaskCommand::Argv(argv) => Some((source, argv.first()?.clone())),
                TaskCommand::Shell(_) => None,
            }
        })
}

/// A row of the picker. Only a task can be picked.
enum Row {
    Task {
        task: Task,
        detail: String,
    },
    /// The typed query as a task of the build tool: always shown, whatever
    /// the query. Not runnable, for what's said.
    Custom {
        text: String,
        run: Result<Task, String>,
    },
    /// There's nothing to run: always shown, whatever the query.
    Empty(String),
    /// A problem with the tasks file.
    Warning(String),
}

impl Row {
    /// The byte ranges of the label that `query`, in lowercase chars, matches:
    /// `None` if the row doesn't match. A task matches by its label, or else
    /// by its source and command, which highlights nothing.
    fn matched(&self, query: &[char]) -> Option<Vec<Range<usize>>> {
        match self {
            Self::Task { task, detail } => match subsequence(query, &task.label) {
                Some(indices) => Some(char_ranges(&task.label, &indices)),
                None => subsequence(query, detail).map(|_| Vec::new()),
            },
            Self::Empty(_) | Self::Custom { .. } => Some(Vec::new()),
            Self::Warning(text) => subsequence(query, text).map(|_| Vec::new()),
        }
    }
}

/// The picker over a root's tasks.
pub(crate) struct TaskPicker {
    /// Only this group's tasks, if any.
    group: Option<Group>,
    /// The root it opened on, whose tasks it lists.
    root: Option<PathBuf>,
    workbench: WeakEntity<Workbench>,
    /// What was focused when it opened, for when no task takes the focus.
    previous_focus: Option<FocusHandle>,
    rows: Vec<Row>,
    /// What runs a typed query: the source and program of the detected Gradle
    /// or Maven tasks, if the root has any.
    runner: Option<(Source, String)>,
    /// The rows matching the query, with the label's ranges it matched.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    query: String,
    /// Whether the list is read.
    loaded: bool,
    /// Reading the list.
    _reading: Option<gpui::Task<()>>,
}

impl TaskPicker {
    /// Shows the list once `reading` has it.
    fn read(&mut self, reading: gpui::Task<Listing>, cx: &mut Context<Picker<Self>>) {
        self._reading = Some(cx.spawn(async move |picker, cx| {
            let listing = reading.await;
            let shown = picker.update(cx, |picker, cx| picker.delegate.set_listing(listing, cx));
            shown.ok();
        }));
    }

    /// Makes rows of `listing`: its tasks, of the group if there's one, or a
    /// row saying there are none, and then its problems.
    fn set_listing(&mut self, (tasks, diagnostics): Listing, cx: &mut Context<Picker<Self>>) {
        let group = self.group;
        self.runner = custom_runner(&tasks);
        let tasks =
            (tasks.into_iter()).filter(|task| group.is_none_or(|group| task.group == Some(group)));
        let mut rows: Vec<Row> = tasks
            .map(|task| Row::Task {
                detail: detail(&task),
                task,
            })
            .collect();
        if rows.is_empty() {
            rows.push(Row::Empty(nothing_to_run(group)));
        }
        rows.extend(
            (diagnostics.iter()).map(|diagnostic| Row::Warning(one_line(&diagnostic.message))),
        );
        self.rows = rows;
        self.loaded = true;
        self.refilter(cx);
    }

    /// The row for the query: after the tasks, before the problems.
    fn custom_row(&self, root: &Path) -> Option<Row> {
        let (source, program) = self.runner.as_ref()?;
        if self.query.is_empty() {
            return None;
        }
        let (text, run) = match split_args(&self.query) {
            Ok(args) => {
                let argv: Vec<String> = std::iter::once(program.clone()).chain(args).collect();
                let line = command_line(&TaskCommand::Argv(argv.clone()));
                let task = Task {
                    label: line.clone(),
                    command: TaskCommand::Argv(argv),
                    cwd: root.to_path_buf(),
                    env: Default::default(),
                    group: None,
                    default: false,
                    source: *source,
                    problem_matcher: None,
                };
                (tr!("workbench-task-custom-run", command = line), Ok(task))
            }
            Err(error) => {
                let text = tr!("workbench-task-custom-run", command = program.as_str());
                let why = match error {
                    SplitError::Empty => tr!("workbench-task-custom-empty"),
                    SplitError::Unterminated => tr!("workbench-task-custom-unterminated"),
                };
                (text, Err(why.to_string()))
            }
        };
        Some(Row::Custom {
            text: text.to_string(),
            run,
        })
    }

    /// Matches the rows against the query, selecting the first.
    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        self.rows.retain(|row| !matches!(row, Row::Custom { .. }));
        if let Some(root) = self.root.clone()
            && let Some(row) = self.custom_row(&root)
        {
            let at = (self.rows.iter())
                .position(|row| matches!(row, Row::Warning(_)))
                .unwrap_or(self.rows.len());
            self.rows.insert(at, row);
        }
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, row.matched(&query)?)))
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    /// Focuses what was focused before it opened.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

/// A row as the tests read it.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownRow {
    Task(String, String),
    /// The query's row, with why it can't run.
    Custom(String, Option<String>),
    Empty(String),
    Warning(String),
}

#[cfg(test)]
impl TaskPicker {
    /// The rows matching the query.
    pub(crate) fn shown(&self) -> Vec<ShownRow> {
        (self.matches.iter())
            .map(|(row, _)| match &self.rows[*row] {
                Row::Task { task, detail } => ShownRow::Task(task.label.clone(), detail.clone()),
                Row::Custom { text, run } => {
                    ShownRow::Custom(text.clone(), run.as_ref().err().cloned())
                }
                Row::Empty(text) => ShownRow::Empty(text.clone()),
                Row::Warning(text) => ShownRow::Warning(text.clone()),
            })
            .collect()
    }

    /// The label's ranges that match `ix` highlights.
    pub(crate) fn highlights(&self, ix: usize) -> Vec<Range<usize>> {
        self.matches[ix].1.clone()
    }

    /// Whether the list is read.
    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }
}

impl PickerDelegate for TaskPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The row's place in the list, which is read once per picker.
    fn match_id(&self, ix: usize) -> SharedString {
        format!("task-row-{}", self.matches[ix].0).into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, highlights) = &self.matches[ix];
        let line = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .whitespace_nowrap();
        match &self.rows[*row] {
            Row::Task { task, detail } => {
                let label = HighlightedLabel::new(task.label.clone(), highlights.clone(), &ui);
                line.child(div().flex_none().child(label)).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(ui.muted_text)
                        .child(detail.clone()),
                )
            }
            Row::Custom { text, run } => match run {
                Ok(_) => line.child(text.clone()),
                Err(why) => line
                    .text_color(ui.muted_text)
                    .child(text.clone())
                    .child(why.clone()),
            },
            Row::Empty(text) => line.text_color(ui.muted_text).child(text.clone()),
            Row::Warning(text) => (line.text_color(cx.theme().colors.status.warning))
                .child(div().min_w_0().truncate().child(text.clone())),
        }
        .into_any_element()
    }

    /// The task's label and detail, or the row's note, as shown.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        match &self.rows[self.matches[ix].0] {
            Row::Task { task, detail } => joined_label([task.label.as_str(), detail.as_str()]),
            Row::Custom { text, run: Ok(_) } => text.clone().into(),
            Row::Custom {
                text,
                run: Err(why),
            } => joined_label([text.as_str(), why.as_str()]),
            Row::Empty(text) | Row::Warning(text) => text.clone().into(),
        }
    }

    /// Runs a task, focus going back where it was first for a run that's
    /// refused, unless the window has changed roots and the task is no longer
    /// one of its own. Any other row stays as it is, the picker open.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let task = match &self.rows[self.matches[ix].0] {
            Row::Task { task, .. } | Row::Custom { run: Ok(task), .. } => task.clone(),
            _ => return false,
        };
        self.restore_focus(window, cx);
        if let Some(workbench) = self.workbench.upgrade()
            && workbench.read(cx).root() == self.root.as_deref()
        {
            workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn status(&self) -> Option<SharedString> {
        // Until the list is read.
        (!self.loaded).then(|| tr!("workbench-task-reading"))
    }
}

impl Workbench {
    /// `tasks.run`: shows the picker, whose list follows once it's read.
    pub(crate) fn pick_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reading = read_tasks(self.root().map(Path::to_owned), cx);
        let picker = self.show_task_picker(None, window, cx);
        picker.update(cx, |picker, cx| picker.delegate.read(reading, cx));
    }

    /// `tasks.rerun_last`: runs the last task again, as it was then.
    pub(crate) fn rerun_last_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.last_task(cx) {
            Some(task) => self.run_task(task, window, cx),
            None => toast::show(cx, Toast::info(no_task_run())),
        }
    }

    /// `tasks.run_build`: runs the default build task, once the list is read
    /// and if the root is still the one it's read from, or with no default
    /// shows the picker on the build group.
    pub(crate) fn run_build_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root().map(Path::to_owned);
        let reading = read_tasks(root.clone(), cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let listing = reading.await;
            let shown = workbench.update_in(cx, |workbench, window, cx| {
                if workbench.root() == root.as_deref() {
                    workbench.build_from(listing, window, cx);
                }
            });
            shown.ok();
        })
        .detach();
    }

    fn build_from(&mut self, listing: Listing, window: &mut Window, cx: &mut Context<Self>) {
        match default_build(&listing.0).cloned() {
            Some(task) => self.run_task(task, window, cx),
            None => {
                let picker = self.show_task_picker(Some(Group::Build), window, cx);
                picker.update(cx, |picker, cx| picker.delegate.set_listing(listing, cx));
            }
        }
    }

    /// Shows an empty picker, focused, in place of any other modal.
    fn show_task_picker(
        &mut self,
        group: Option<Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Picker<TaskPicker>> {
        // First, as it gives the focus back to what this picker returns to.
        let modals = self.shell().read(cx).modals().clone();
        modals.update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        let delegate = TaskPicker {
            group,
            root: self.root().map(Path::to_owned),
            workbench: cx.entity().downgrade(),
            previous_focus: window.focused(cx),
            rows: Vec::new(),
            runner: None,
            matches: Vec::new(),
            query: String::new(),
            loaded: false,
            _reading: None,
        };
        let placeholder = match group {
            None => text!("workbench-task-run-placeholder"),
            Some(_) => text!("workbench-task-run-build-placeholder"),
        };
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(TASK_PICKER, picker.clone(), window, cx);
        });
        picker
    }
}

#[cfg(test)]
impl Workbench {
    /// The task picker in the modal layer, if it's the modal showing.
    pub(crate) fn task_picker(&self, cx: &App) -> Option<Entity<Picker<TaskPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == TASK_PICKER)?;
        layer.modal_view()?.clone().downcast().ok()
    }
}
