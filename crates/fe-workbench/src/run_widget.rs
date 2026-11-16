//! The title bar's run widget (`docs/design/design-system.md`, Workbench
//! anatomy): in the right group, before search, the chosen task's label and
//! a chevron, which opens a picker of the root's tasks (the choice is kept
//! per project for the life of the window); a play button that runs it; and,
//! while it runs, a stop button that stops it. A running task shows the
//! spinner icon, and one whose last run failed the error glyph, in place of
//! the chevron. With neither tasks nor launch configurations the widget
//! draws nothing.
//!
//! It debugs too (card D8b, [`crate::run_widget_debug`]): the picker lists
//! the root's launch configurations after its tasks, the choice may be one
//! of them, a Debug button starts it (or, a task chosen, Debug… shows
//! `debug.start`'s picker), and while a session runs the widget is the
//! session's controls.
//!
//! The widget is an entity the title bar's right slot holds ([`install`]).
//! It lists the workbench's root's tasks and launch configurations, read
//! when the root changes and when the file watcher says a file that
//! defines tasks or configurations did (and again, at once, as play and the
//! picker use them). The choice is the workbench's, per root, saved with
//! its layout (`run_tasks`); with none made it's the default build task,
//! else the first task, else the first configuration. How the task goes is
//! the terminal panel's: the status and stop follow any run of it, from
//! play or from `tasks.run`, `tasks.run_build` or `tasks.rerun_last`, and a
//! rerun stays tracked. How a session goes is the window's debugger's.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_config::debug_settings::DebugSettings;
use fe_project::FsChange;
use fe_ui::{Button, Icon, IconButton, Tooltip};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    WeakEntity, Window, div, px,
};
use kiln_l10n::{Text, tr};
use kiln_shell::{TitleBarItems, TitleBarSlot};

use crate::{
    RootChanged, Workbench,
    debug_session::debugger_of,
    launch_defs::LaunchConfig,
    pane_toolbar,
    project_panel::PanelEvent,
    run_widget_debug::{self as debug, RunChoice},
    task_commands::{self, default_build},
    task_defs::Task,
    terminal_panel::{TaskRun, TerminalPanel},
};

/// The label button's id.
const LABEL_ID: &str = "title-run";

/// The gap between the widget's buttons.
const GAP: gpui::Pixels = px(2.);

/// The widest the label gets: a longer one ends in an ellipsis.
const LABEL_MAX_WIDTH: gpui::Pixels = px(200.);

/// How a task the widget ran has gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Run {
    Idle,
    Running,
    Failed,
}

impl Run {
    /// The icon after the label: the chevron that opens the picker, or what
    /// the run has to show.
    pub(crate) fn icon(self) -> Icon {
        match self {
            Self::Idle => Icon::ChevronDown,
            Self::Running => Icon::LoaderCircle,
            Self::Failed => Icon::CircleX,
        }
    }
}

/// What waits for fresh tasks.
#[derive(Clone, Copy)]
enum Action {
    Play,
    Pick,
}

/// What the root's choice is, among what's read.
enum Chosen<'a> {
    Task(&'a Task),
    Config(&'a LaunchConfig),
}

/// What the bar shows, to draw it again only when that changes.
#[derive(Clone, Debug, PartialEq)]
enum Look {
    /// A task, how its run goes, and whether Debug… shows (the root has a
    /// launch configuration).
    Task(String, Run, bool),
    /// A launch configuration, and why it can't start if it can't.
    Config(String, Option<SharedString>),
    /// A debug session: its label's name, whether it's stopped, and its
    /// buttons and whether each is enabled.
    Session(SharedString, bool, Vec<(&'static str, bool)>),
}

pub(crate) struct RunWidget {
    workbench: WeakEntity<Workbench>,
    /// The listed root's tasks, as last read.
    tasks: Vec<Task>,
    /// Its launch configurations, as last read: read apart from the tasks,
    /// so a slow read (jdtls's main classes) never holds up a task.
    configs: Vec<LaunchConfig>,
    /// The root `configs` are of.
    configs_root: Option<PathBuf>,
    config_reading: Option<gpui::Task<()>>,
    /// The configuration loads started, and the newest one applied.
    config_loads: u64,
    config_applied: u64,
    /// What the configurations depend on beside the files, as last read:
    /// the active editor's language (detection lists the current Python
    /// file) and the `[debug]` settings (the user's adapters).
    inputs: Option<(Option<String>, DebugSettings)>,
    /// The picker, while it shows, which configurations read after it
    /// opened are handed to.
    picker: Option<debug::RunPickerHandle>,
    /// The root `tasks` are of: nothing shows or runs while it isn't the
    /// workbench's.
    listed_root: Option<PathBuf>,
    reading: Option<gpui::Task<()>>,
    /// The loads started, and the newest one applied: a load that lands
    /// after a newer one has is dropped.
    loads: u64,
    applied: u64,
    /// Play or the picker, waiting for tasks read afresh; the newest
    /// request is the only one that acts.
    acting: Option<gpui::Task<()>>,
    actions: u64,
    /// What the bar last showed, to redraw it only when that changes.
    shown: Option<Look>,
    /// To the terminal panel, whose tabs say how runs go, whoever started
    /// them; to the workbench, for the choice (kept in its layout) and the
    /// root; and to the project panel's file watching.
    _following: Vec<Subscription>,
    _panel: Option<Subscription>,
}

/// Puts the widget before `items`' right slot, and has it read the tasks.
pub(crate) fn install(
    items: &mut TitleBarItems,
    workbench: &Entity<Workbench>,
    terminal: &Entity<TerminalPanel>,
    cx: &mut Context<Workbench>,
) {
    let widget = cx.new(|cx| RunWidget {
        workbench: workbench.downgrade(),
        tasks: Vec::new(),
        configs: Vec::new(),
        configs_root: None,
        config_reading: None,
        config_loads: 0,
        config_applied: 0,
        inputs: None,
        picker: None,
        listed_root: None,
        reading: None,
        loads: 0,
        applied: 0,
        acting: None,
        actions: 0,
        shown: None,
        _following: vec![
            cx.observe(terminal, |widget: &mut RunWidget, _, cx| widget.changed(cx)),
            cx.observe(workbench, |widget: &mut RunWidget, _, cx| {
                widget.changed(cx)
            }),
            cx.subscribe(
                workbench,
                |widget: &mut RunWidget, _, _: &RootChanged, cx| {
                    widget.root_changed(cx);
                },
            ),
        ],
        _panel: None,
    });
    #[cfg(test)]
    LATEST.with_borrow_mut(|latest| *latest = Some(widget.downgrade()));
    // The workbench is being updated now: it can't be read until then.
    let starting = widget.clone();
    let workbench = workbench.clone();
    cx.defer(move |cx| {
        starting.update(cx, |widget, cx| {
            // Its settings changing changes what can start.
            if let Some(debugger) = debugger_of(&workbench, cx) {
                let following = cx.observe(&debugger, |widget: &mut RunWidget, _, cx| {
                    widget.changed(cx);
                });
                widget._following.push(following);
            }
            widget.root_changed(cx);
        });
    });
    let rest = items.right.take();
    let slot: TitleBarSlot = Rc::new(move |window, cx| {
        debug::drawing();
        let shown = widget.read(cx).element(&widget, cx);
        let rest = rest.as_ref().map(|rest| rest(window, cx));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(GAP)
            .children(shown)
            .children(rest)
            .into_any_element()
    });
    items.right = Some(slot);
}

#[cfg(test)]
thread_local! {
    /// The widget installed last, for tests to press play on without a click's
    /// waiting for the read.
    static LATEST: std::cell::RefCell<Option<WeakEntity<RunWidget>>> = const { std::cell::RefCell::new(None) };
}

/// Presses play on the widget installed last, doing no more than that: the
/// read it waits for isn't run.
#[cfg(test)]
pub(crate) fn press_play(window: &mut Window, cx: &mut App) {
    let widget = LATEST
        .with_borrow(Clone::clone)
        .expect("a widget is installed");
    widget
        .update(cx, |widget, cx| widget.play(window, cx))
        .expect("the widget is alive");
}

/// Whether a batch from the file watcher touched a file that defines or
/// implies tasks or launch configurations in `root`, the `.tenmoku` folder
/// that holds the tasks and launch files, `.vscode/launch.json`, or a
/// folder above them (made, removed or renamed), or lost track.
fn touches_tasks(changes: &[FsChange], root: &Path) -> bool {
    const FILES: [&str; 12] = [
        "tasks.toml",
        "Cargo.toml",
        "package.json",
        "GNUmakefile",
        "makefile",
        "Makefile",
        "justfile",
        "Justfile",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lockb",
        "bun.lock",
    ];
    let folder = root.join(".tenmoku");
    let launch_json = root.join(crate::launch_defs::LAUNCH_JSON);
    // Cargo's binary targets, which detection lists.
    let (main, bins) = (root.join("src/main.rs"), root.join("src/bin"));
    changes.iter().any(|change| match change {
        FsChange::Rescan => true,
        FsChange::Path(path) => {
            path.starts_with(&folder)
                || folder.starts_with(path)
                || launch_json.starts_with(path)
                || main.starts_with(path)
                || path.starts_with(&bins)
                || (path.parent() == Some(root))
                    && (path.file_name().and_then(|name| name.to_str()))
                        .is_some_and(|name| FILES.contains(&name))
        }
    })
}

impl RunWidget {
    /// The root changed (or the widget starts): the old root's tasks go at
    /// once, the new root's are read, and its project panel is followed.
    fn root_changed(&mut self, cx: &mut Context<Self>) {
        self.tasks.clear();
        self.configs.clear();
        self.configs_root = None;
        self.config_reading = None;
        self.config_applied = self.config_loads;
        self.listed_root = None;
        self.reading = None;
        // A request for the old root's tasks acts on nothing.
        self.acting = None;
        self.actions += 1;
        self.applied = self.loads;
        self._panel = self.workbench.upgrade().map(|workbench| {
            let panel = workbench.read(cx).panel().clone();
            cx.subscribe(
                &panel,
                |widget: &mut RunWidget, _, event: &PanelEvent, cx| {
                    if let PanelEvent::FsChanged(changes) = event
                        && let Some(root) = widget.root(cx)
                        && touches_tasks(changes, &root)
                    {
                        widget.refresh(cx);
                    }
                },
            )
        });
        self.changed(cx);
        self.refresh(cx);
    }

    /// Whether tasks can be read here: not in tests that give the app no
    /// project to read.
    fn can_read(cx: &App) -> bool {
        #[cfg(test)]
        if !cx.has_global::<task_commands::TestFiles>() {
            return false;
        }
        let _ = cx;
        true
    }

    /// Reads the root's tasks and, apart, its launch configurations, off
    /// the main thread.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if !Self::can_read(cx) {
            return;
        }
        let root = self.root(cx);
        self.loads += 1;
        let load = self.loads;
        let reading = task_commands::read_tasks(root.clone(), cx);
        self.reading = Some(cx.spawn(async move |widget, cx| {
            let (tasks, _) = reading.await;
            widget
                .update(cx, |widget, cx| widget.loaded(load, root, tasks, cx))
                .ok();
        }));
        self.refresh_configs(cx);
    }

    /// Reads the root's launch configurations, off the main thread.
    fn refresh_configs(&mut self, cx: &mut Context<Self>) {
        if !Self::can_read(cx) {
            return;
        }
        let (Some(root), Some(workbench)) = (self.root(cx), self.workbench.upgrade()) else {
            return;
        };
        self.inputs = Some(self.config_inputs(cx));
        self.config_loads += 1;
        let load = self.config_loads;
        let reading = debug::read_configs(&workbench, cx);
        self.config_reading = Some(cx.spawn(async move |widget, cx| {
            let configs = reading.await;
            widget
                .update(cx, |widget, cx| {
                    widget.configs_loaded(load, root, configs, cx)
                })
                .ok();
        }));
    }

    /// Takes the tasks of load `load` of `root`, unless the root has changed
    /// or a newer load is in.
    fn loaded(
        &mut self,
        load: u64,
        root: Option<PathBuf>,
        tasks: Vec<Task>,
        cx: &mut Context<Self>,
    ) {
        if self.root(cx) != root || load <= self.applied {
            return;
        }
        self.applied = load;
        self.tasks = tasks;
        self.listed_root = root;
        self.changed(cx);
    }

    /// Takes the configurations of load `load` of `root`, unless the root
    /// has changed or a newer load is in, and hands them to the picker if
    /// it shows.
    fn configs_loaded(
        &mut self,
        load: u64,
        root: PathBuf,
        configs: Vec<LaunchConfig>,
        cx: &mut Context<Self>,
    ) {
        if self.root(cx).as_ref() != Some(&root) || load <= self.config_applied {
            return;
        }
        self.config_applied = load;
        self.configs = configs;
        self.configs_root = Some(root);
        if let Some(picker) = &self.picker
            && !debug::update_run_picker(picker, &self.configs, cx)
        {
            self.picker = None;
        }
        self.changed(cx);
    }

    /// The root's configurations, once read: none before.
    fn configs(&self) -> &[LaunchConfig] {
        match self.configs_root.is_some() && self.configs_root == self.listed_root {
            true => &self.configs,
            false => &[],
        }
    }

    /// Reads the root's tasks afresh, off the main thread, then does `what`
    /// with them if the root is still the same and no newer request has been
    /// made: a file written since the watcher's batch is in them.
    fn act(&mut self, what: Action, window: &mut Window, cx: &mut Context<Self>) {
        if !Self::can_read(cx) {
            return;
        }
        let Some(root) = self.root(cx) else {
            return;
        };
        self.actions += 1;
        let request = self.actions;
        self.loads += 1;
        let load = self.loads;
        let previous_focus = window.focused(cx);
        let reading = task_commands::read_tasks(Some(root.clone()), cx);
        if let Action::Pick = what {
            // The picker shows the configurations read so far, then these.
            self.refresh_configs(cx);
        }
        self.acting = Some(cx.spawn_in(window, async move |widget, cx| {
            let (tasks, _) = reading.await;
            widget
                .update_in(cx, |widget, window, cx| {
                    if widget.actions != request || widget.root(cx).as_ref() != Some(&root) {
                        return;
                    }
                    widget.loaded(load, Some(root), tasks, cx);
                    match what {
                        Action::Play => widget.play_now(window, cx),
                        Action::Pick => widget.pick_now(previous_focus, window, cx),
                    }
                })
                .ok();
        }));
    }

    fn root(&self, cx: &App) -> Option<PathBuf> {
        let workbench = self.workbench.upgrade()?;
        workbench.read(cx).root().map(ToOwned::to_owned)
    }

    /// What the configurations depend on beside the files: the active
    /// editor's language and the `[debug]` settings.
    fn config_inputs(&self, cx: &App) -> (Option<String>, DebugSettings) {
        let Some(workbench) = self.workbench.upgrade() else {
            return Default::default();
        };
        let editor = workbench.read(cx).active_editor(cx);
        let syntax = editor.and_then(|editor| editor.read(cx).syntax());
        let language = syntax.map(|syntax| syntax.language().name().to_owned());
        let settings = debugger_of(&workbench, cx)
            .map(|debugger| debugger.read(cx).settings().clone())
            .unwrap_or_default();
        (language, settings)
    }

    /// What the bar shows, if it changed, drawn again; and the
    /// configurations read again if what they depend on changed.
    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.inputs.is_some() && self.inputs.as_ref() != Some(&self.config_inputs(cx)) {
            self.refresh_configs(cx);
        }
        let now = self.look(cx);
        if now == self.shown {
            return;
        }
        self.shown = now;
        if let Some(workbench) = self.workbench.upgrade() {
            let shell = workbench.read(cx).shell().clone();
            shell.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// What's shown: the root's choice if it's still a task or a
    /// configuration, else the default build task, else the first task,
    /// else the first configuration. None until the root's are read.
    fn choice(&self, cx: &App) -> Option<Chosen<'_>> {
        let workbench = self.workbench.upgrade()?;
        let workbench = workbench.read(cx);
        let root = workbench.root()?;
        if self.listed_root.as_deref() != Some(root) {
            return None;
        }
        let chosen = match workbench.run_task_choice(root) {
            Some(RunChoice::Task(label)) => (self.tasks.iter())
                .find(|task| task.label == label)
                .map(Chosen::Task),
            Some(RunChoice::Debug(name)) => (self.configs().iter())
                .find(|config| config.name == name)
                .map(Chosen::Config),
            None => None,
        };
        chosen
            .or_else(|| default_build(&self.tasks).map(Chosen::Task))
            .or_else(|| self.tasks.first().map(Chosen::Task))
            .or_else(|| self.configs().first().map(Chosen::Config))
    }

    /// The task shown, if a task is.
    fn chosen_task(&self, cx: &App) -> Option<&Task> {
        match self.choice(cx)? {
            Chosen::Task(task) => Some(task),
            Chosen::Config(_) => None,
        }
    }

    /// How the task `task` last went, whoever ran it.
    fn run_of(&self, task: &Task, cx: &App) -> Option<Run> {
        let workbench = self.workbench.upgrade()?;
        let terminal = workbench.read(cx).terminal().read(cx);
        let run = match terminal.task_run(self.listed_root.as_deref()?, &task.label) {
            Some(TaskRun::Running) => Run::Running,
            Some(TaskRun::Failed) => Run::Failed,
            Some(TaskRun::Passed) | None => Run::Idle,
        };
        Some(run)
    }

    /// What the bar shows: the session's controls while one runs, else the
    /// choice.
    fn look(&self, cx: &App) -> Option<Look> {
        let workbench = self.workbench.upgrade()?;
        if let Some(debugger) = debugger_of(&workbench, cx)
            && let Some((name, stopped, buttons)) = debug::session_look(debugger.read(cx))
        {
            return Some(Look::Session(name, stopped, buttons));
        }
        match self.choice(cx)? {
            Chosen::Task(task) => {
                let run = self.run_of(task, cx)?;
                Some(Look::Task(
                    task.label.clone(),
                    run,
                    !self.configs().is_empty(),
                ))
            }
            Chosen::Config(config) => {
                Some(Look::Config(config.name.clone(), debug::blocked(config)))
            }
        }
    }

    /// Play: runs the task shown, as its file has it by the time it's read.
    fn play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Action::Play, window, cx);
    }

    fn play_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.chosen_task(cx).cloned() else {
            return;
        };
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
        self.changed(cx);
    }

    /// Stop: drops the task shown's running sessions.
    fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.listed_root.clone() else {
            return;
        };
        let Some(label) = self.chosen_task(cx).map(|task| task.label.clone()) else {
            return;
        };
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let terminal = workbench.read(cx).terminal().clone();
        terminal.update(cx, |terminal, cx| {
            terminal.stop_task(&root, &label, window, cx)
        });
    }

    /// The chevron: shows the picker of the root's tasks, once they're read.
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Action::Pick, window, cx);
    }

    fn pick_now(
        &mut self,
        previous_focus: Option<gpui::FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let widget = cx.entity().downgrade();
        let root = self.root(cx);
        let choose: debug::OnChoose = Rc::new(move |choice, _, cx| {
            let root = root.clone();
            widget
                .update(cx, |widget, cx| {
                    if widget.root(cx) == root
                        && let (Some(root), Some(workbench)) = (root, widget.workbench.upgrade())
                    {
                        workbench.update(cx, |workbench, cx| {
                            workbench.choose_run_task(&root, choice, cx)
                        });
                        widget.changed(cx);
                    }
                })
                .ok();
        });
        self.picker = Some(debug::show_run_picker(
            &workbench,
            &self.tasks,
            self.configs(),
            choose,
            previous_focus,
            window,
            cx,
        ));
    }

    /// The session label: the session menu, whose Choose Task or
    /// Configuration… is the chevron's picker.
    fn session_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let widget = cx.entity().downgrade();
        let choose = Rc::new(move |window: &mut Window, cx: &mut App| {
            widget.update(cx, |widget, cx| widget.pick(window, cx)).ok();
        });
        debug::show_session_menu(&workbench, choose, window, cx);
    }

    /// The widget's elements, none with neither tasks nor configurations.
    fn element(&self, entity: &Entity<Self>, cx: &App) -> Option<AnyElement> {
        let look = self.look(cx)?;
        let widget = entity.downgrade();
        if let Look::Session(..) = look {
            let debugger = debugger_of(&self.workbench.upgrade()?, cx)?;
            let on_label = move |window: &mut Window, cx: &mut App| {
                (widget.update(cx, |widget, cx| widget.session_menu(window, cx))).ok();
            };
            let debugger = debugger.read(cx);
            return Some(debug::session_element(
                debugger,
                on_label,
                LABEL_MAX_WIDTH,
                cx,
            ));
        }
        let choose = match self.configs().is_empty() {
            true => tr!("workbench-run-choose"),
            false => tr!("workbench-debug-run-choose"),
        };
        let pick = {
            let widget = widget.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                widget.update(cx, |widget, cx| widget.pick(window, cx)).ok();
            }
        };
        let (label, actions) = match &look {
            Look::Task(task, run, debug) => {
                let name = match run {
                    Run::Idle => tr!("workbench-run-task", name = task),
                    Run::Running => tr!("workbench-run-task-running", name = task),
                    Run::Failed => tr!("workbench-run-task-failed", name = task),
                };
                #[cfg(test)]
                crate::title_bar::DRAWN
                    .with_borrow_mut(|drawn| drawn.push(("title-run", name.clone())));
                let label = Button::new(LABEL_ID, Text::Verbatim(task.clone().into()))
                    .ghost()
                    .icon(run.icon())
                    .name(name);
                let running = *run == Run::Running;
                let (id, icon, title) = match running {
                    true => ("title-run-stop", Icon::Square, tr!("workbench-run-stop")),
                    false => ("title-run-play", Icon::Play, tr!("workbench-run-play")),
                };
                #[cfg(test)]
                crate::title_bar::DRAWN.with_borrow_mut(|drawn| drawn.push((id, title.clone())));
                let action = IconButton::new(id, icon, title.clone(), &fe_ui::ui_theme(cx))
                    .tooltip(tooltip(title, None))
                    .on_click(move |_, window, cx| {
                        widget
                            .update(cx, |widget, cx| match running {
                                true => widget.stop(window, cx),
                                false => widget.play(window, cx),
                            })
                            .ok();
                    });
                let mut actions = vec![action.into_any_element()];
                if *debug {
                    actions.push(debug::debug_pick_button(cx));
                }
                (label, actions)
            }
            Look::Config(name, _) => {
                let label_name = debug::config_label_name(name);
                #[cfg(test)]
                crate::title_bar::DRAWN
                    .with_borrow_mut(|drawn| drawn.push(("title-run", label_name.clone())));
                let label = Button::new(LABEL_ID, Text::Verbatim(name.clone().into()))
                    .ghost()
                    .icon(Run::Idle.icon())
                    .after_icon(debug::label_bug(cx))
                    .name(label_name);
                let config = (self.configs().iter()).find(|config| &config.name == name)?;
                (label, vec![debug::debug_button(config, cx)])
            }
            Look::Session(..) => return None,
        };
        let label = label
            .flex_row_reverse()
            .flex_shrink(1.)
            .min_w_0()
            .max_w(LABEL_MAX_WIDTH)
            .debug_selector(|| "title-run".into())
            .tooltip(tooltip(choose, Some("tasks.run")))
            .on_click(pick);
        Some(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(GAP)
                .child(label)
                .children(actions)
                .into_any_element(),
        )
    }
}

/// A tooltip of `title` and `command`'s key, as the keymap has it when it
/// shows.
fn tooltip(
    title: SharedString,
    command: Option<&'static str>,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| {
        let shortcut = command.and_then(|command| pane_toolbar::shortcut(command, cx));
        let tooltip = Tooltip::new(title.clone(), shortcut, &fe_ui::ui_theme(cx));
        #[cfg(test)]
        crate::title_bar::LAST_TOOLTIP.with_borrow_mut(|last| *last = Some(tooltip.text()));
        cx.new(|_| tooltip).into()
    }
}
