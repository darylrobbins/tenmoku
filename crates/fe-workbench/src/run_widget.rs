//! The title bar's run widget (`docs/design/design-system.md`, Workbench
//! anatomy): in the right group, before search, the chosen task's label and
//! a chevron, which opens a picker of the root's tasks (the choice is kept
//! per project for the life of the window); a play button that runs it; and,
//! while it runs, a stop button that stops it. A running task shows the
//! spinner icon, and one whose last run failed the error glyph, in place of
//! the chevron. With no tasks the widget draws nothing.
//!
//! The widget is an entity the title bar's right slot holds ([`install`]).
//! It lists the workbench's root's tasks, read when the root changes and
//! when the file watcher says a file that defines tasks did (and again, at
//! once, as play and the picker use them). The choice is the workbench's, per
//! root, saved with its layout (`run_tasks`); with none made it's the default
//! build task, else the first. How the task goes is the terminal panel's:
//! the status and stop follow any run of it, from play or from `tasks.run`,
//! `tasks.run_build` or `tasks.rerun_last`, and a rerun stays tracked.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_project::FsChange;
use fe_ui::{Button, Icon, IconButton, Tooltip};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    WeakEntity, Window, div, px,
};
use kiln_l10n::{Text, tr};
use kiln_shell::{ModalId, TitleBarItems, TitleBarSlot};

use crate::{
    RootChanged, Workbench, pane_toolbar,
    picker::{Picker, PickerDelegate},
    project_panel::PanelEvent,
    task_commands::{self, default_build},
    task_defs::Task,
    terminal_panel::{TaskRun, TerminalPanel},
};

/// The picker's name in the modal layer.
const PICKER: ModalId = ModalId("run-widget-tasks");

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

pub(crate) struct RunWidget {
    workbench: WeakEntity<Workbench>,
    /// The listed root's tasks, as last read.
    tasks: Vec<Task>,
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
    shown: Option<(String, Run)>,
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
    cx.defer(move |cx| {
        starting.update(cx, |widget, cx| widget.root_changed(cx));
    });
    let rest = items.right.take();
    let slot: TitleBarSlot = Rc::new(move |window, cx| {
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
/// implies tasks in `root`, the `.tenmoku` folder that holds the tasks file
/// or a folder above it (made, removed or renamed), or lost track.
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
    changes.iter().any(|change| match change {
        FsChange::Rescan => true,
        FsChange::Path(path) => {
            path.starts_with(&folder)
                || folder.starts_with(path)
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

    /// Reads the root's tasks, off the main thread.
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

    /// What the bar shows, if it changed, drawn again.
    fn changed(&mut self, cx: &mut Context<Self>) {
        let now = self.view(cx).map(|(task, run)| (task.label.clone(), run));
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

    /// The task shown: the root's choice if it's still a task, else the
    /// default build task, else the first. None until the root's tasks are
    /// read.
    fn choice(&self, cx: &App) -> Option<&Task> {
        let workbench = self.workbench.upgrade()?;
        let workbench = workbench.read(cx);
        let root = workbench.root()?;
        if self.listed_root.as_deref() != Some(root) {
            return None;
        }
        let chosen = workbench.run_task_choice(root);
        let chosen = chosen.and_then(|label| self.tasks.iter().find(|task| task.label == label));
        chosen
            .or_else(|| default_build(&self.tasks))
            .or_else(|| self.tasks.first())
    }

    /// The task shown and how it last went, whoever ran it.
    fn view(&self, cx: &App) -> Option<(&Task, Run)> {
        let task = self.choice(cx)?;
        let workbench = self.workbench.upgrade()?;
        let terminal = workbench.read(cx).terminal().read(cx);
        let run = match terminal.task_run(self.listed_root.as_deref()?, &task.label) {
            Some(TaskRun::Running) => Run::Running,
            Some(TaskRun::Failed) => Run::Failed,
            Some(TaskRun::Passed) | None => Run::Idle,
        };
        Some((task, run))
    }

    /// Play: runs the task shown, as its file has it by the time it's read.
    fn play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Action::Play, window, cx);
    }

    fn play_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.choice(cx).cloned() else {
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
        let Some(label) = self.choice(cx).map(|task| task.label.clone()) else {
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
        let delegate = TaskChoice {
            widget: cx.entity().downgrade(),
            root: self.root(cx),
            labels: self.tasks.iter().map(|task| task.label.clone()).collect(),
            matches: Vec::new(),
            previous_focus,
        };
        let placeholder = kiln_l10n::text!("workbench-run-choose-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.delegate.refilter(String::new());
            picker.show(window, cx);
        });
        let shell = workbench.read(cx).shell().clone();
        shell.update(cx, |shell, cx| shell.show_modal(PICKER, picker, window, cx));
    }

    /// The widget's elements, none with no tasks.
    fn element(&self, entity: &Entity<Self>, cx: &App) -> Option<AnyElement> {
        let (task, run) = self.view(cx)?;
        let name = match run {
            Run::Idle => tr!("workbench-run-task", name = &task.label),
            Run::Running => tr!("workbench-run-task-running", name = &task.label),
            Run::Failed => tr!("workbench-run-task-failed", name = &task.label),
        };
        #[cfg(test)]
        crate::title_bar::DRAWN.with_borrow_mut(|drawn| drawn.push(("title-run", name.clone())));
        let widget = entity.downgrade();
        let label = Button::new(LABEL_ID, Text::Verbatim(task.label.clone().into()))
            .ghost()
            .icon(run.icon())
            .name(name)
            .flex_row_reverse()
            .flex_shrink(1.)
            .min_w_0()
            .max_w(LABEL_MAX_WIDTH)
            .debug_selector(|| "title-run".into())
            .tooltip(tooltip(tr!("workbench-run-choose"), Some("tasks.run")))
            .on_click({
                let widget = widget.clone();
                move |_, window, cx| {
                    widget.update(cx, |widget, cx| widget.pick(window, cx)).ok();
                }
            });
        let running = run == Run::Running;
        let (id, icon, title) = match running {
            true => ("title-run-stop", Icon::Square, tr!("workbench-run-stop")),
            false => ("title-run-play", Icon::Play, tr!("workbench-run-play")),
        };
        #[cfg(test)]
        crate::title_bar::DRAWN.with_borrow_mut(|drawn| drawn.push((id, title.clone())));
        let action = IconButton::new(id, icon, &fe_ui::ui_theme(cx))
            .name(title.clone())
            .tooltip(tooltip(title, None))
            .on_click(move |_, window, cx| {
                widget
                    .update(cx, |widget, cx| match running {
                        true => widget.stop(window, cx),
                        false => widget.play(window, cx),
                    })
                    .ok();
            });
        Some(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(GAP)
                .child(label)
                .child(action)
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

/// The picker over the root's tasks: confirming one chooses it.
struct TaskChoice {
    widget: WeakEntity<RunWidget>,
    /// The root it opened on: a choice is the root's.
    root: Option<PathBuf>,
    labels: Vec<String>,
    /// The labels' indices matching the query.
    matches: Vec<usize>,
    /// What was focused when it opened.
    previous_focus: Option<gpui::FocusHandle>,
}

impl TaskChoice {
    fn refilter(&mut self, query: String) {
        let query = query.to_lowercase();
        self.matches = (self.labels.iter().enumerate())
            .filter(|(_, label)| label.to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect();
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

impl PickerDelegate for TaskChoice {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.refilter(query);
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        format!("run-task-row-{}", self.matches[ix]).into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        let label = self.labels[self.matches[ix]].clone();
        div().truncate().child(label).into_any_element()
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let label = self.labels[self.matches[ix]].clone();
        self.restore_focus(window, cx);
        let root = self.root.clone();
        self.widget
            .update(cx, |widget, cx| {
                if widget.root(cx) == root
                    && let (Some(root), Some(workbench)) = (root, widget.workbench.upgrade())
                {
                    workbench.update(cx, |workbench, cx| {
                        workbench.choose_run_task(&root, label, cx)
                    });
                    widget.changed(cx);
                }
            })
            .ok();
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }
}
