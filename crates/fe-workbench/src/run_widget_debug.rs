//! The title bar's run widget debugs (docs/decisions/r1b9dw3g0g-debugger.md
//! §13 "B3 design first"; `docs/design/design-system.md`, Debugger, "The
//! run widget"; card D8b). The widget itself is `run_widget.rs`'s; this is
//! what it gains for the debugger.
//!
//! - **One choice, a task or a launch configuration** ([`RunChoice`]),
//!   kept per root in the window's layout as the task choice was. The
//!   root's launch configurations (D4: the launch file's, `launch.json`'s
//!   and the detected ones) are read with its tasks
//!   ([`read_configs`]), as `debug.start`'s picker reads them.
//! - **The picker** ([`show_run_picker`], Choose Task or Configuration…)
//!   lists the tasks, then the configurations, each group under a faint
//!   heading once there are configurations; a configuration's row has the
//!   `bug` icon and its source as a faint suffix (`launch.toml`,
//!   `launch.json`, detected). Typing filters both groups. A heading is a
//!   row that picks nothing.
//! - **Idle, a task chosen**: ▷ (■ while it runs), then, if the root has a
//!   configuration, 🐞 Debug…, which dispatches `debug.start`: its own
//!   picker, at the configurations ([`debug_pick_button`]). **A
//!   configuration chosen**: its label carries the `bug` icon, and only 🐞
//!   Debug shows ([`debug_button`]), which dispatches `debug.start` with
//!   the configuration's name. One that can't start leaves it disabled,
//!   its tooltip saying why.
//! - **While a session runs** (starting, or not yet ended) the widget is
//!   the session's controls ([`session_element`]), as D0 places them and
//!   as the floating toolbar (`debug_toolbar.rs`, D8) has them where
//!   there's no title bar: the label names the configuration with the
//!   session's state icon (a spinner while it runs, `pause` while
//!   stopped), and opens the session menu ([`show_session_menu`]); then
//!   Continue or Pause (both while uncertain), Step Over, Step Into, Step
//!   Out, 6 px, Restart and Stop, each dispatching its `debug.*` command
//!   and enabled as the session's control-state query says. The toolbar
//!   draws only without a title bar, so a window never has both.
//! - **Names.** Every control has an accessible name: the design's "Task:
//!   ‹label›", "Debug configuration: ‹name›", "Debug session: ‹name›,
//!   running" and the buttons' (Debug, Debug…, Continue, …). Tooltips add
//!   the key that runs the button's command.

use std::rc::Rc;

use fe_command::{ContextFrame, Invocation, Source, Value};
use fe_gpui::commands::RunCommand;
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton, Tooltip, height};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    Styled as _, WeakEntity, Window, div, px, svg,
};
use kiln_l10n::{Text, text, tr, verbatim};
use kiln_shell::{ModalId, PanelId, picker::joined_label};

use crate::{
    Workbench,
    debug_session::{DEBUG_SESSION, Debugger},
    debug_toolbar,
    launch_defs::{self, LaunchConfig},
    picker::{Picker, PickerDelegate},
    task_defs::Task,
};

/// The run widget's picker's name in the modal layer.
pub(crate) const RUN_PICKER: ModalId = ModalId("run-widget-tasks");
/// The session menu's.
pub(crate) const SESSION_MENU: ModalId = ModalId("run-widget-session");

/// The Debug button's id, with a configuration chosen.
pub(crate) const DEBUG_ID: &str = "title-run-debug";
/// The Debug… button's, with a task chosen.
pub(crate) const DEBUG_PICK_ID: &str = "title-run-debug-pick";
/// The session label's.
pub(crate) const SESSION_ID: &str = verbatim!("title-run-session");

/// The key a configuration's choice is saved under in the layout.
const DEBUG_KEY: &str = verbatim!("debug");

/// Between the session's buttons.
const GAP: gpui::Pixels = px(2.);
/// Before Restart.
const GROUP_GAP: gpui::Pixels = px(6.);

/// What the run widget shows in a root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RunChoice {
    /// A task, by its label.
    Task(String),
    /// A launch configuration, by its name.
    Debug(String),
}

impl RunChoice {
    /// As the layout saves it: a task as its label, as before there were
    /// configurations to choose, and a configuration as `{"debug": name}`.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::Task(label) => Value::from(label.as_str()),
            Self::Debug(name) => Value::from_iter([(DEBUG_KEY, Value::from(name.as_str()))]),
        }
    }

    /// As [`RunChoice::to_value`] saved it.
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(label) => Some(Self::Task(label.clone())),
            Value::Object(object) => Some(Self::Debug(object.get(DEBUG_KEY)?.as_str()?.to_owned())),
            _ => None,
        }
    }
}

/// Reads `workbench`'s root's launch configurations off the main thread,
/// as `debug.start`'s picker reads them.
pub(crate) fn read_configs(
    workbench: &Entity<Workbench>,
    cx: &mut App,
) -> gpui::Task<Vec<LaunchConfig>> {
    #[cfg(test)]
    if cx.has_global::<HoldConfigReads>() {
        return cx.background_spawn(futures::future::pending());
    }
    let reading = workbench.update(cx, |workbench, cx| workbench.read_launch_configs(cx));
    cx.background_spawn(async move { reading.await.configs })
}

/// While set, configuration reads never finish: for tests that tasks don't
/// wait on them.
#[cfg(test)]
pub(crate) struct HoldConfigReads;

#[cfg(test)]
impl gpui::Global for HoldConfigReads {}

/// Where a configuration came from, as its picker row's suffix says it.
fn source_name(source: launch_defs::Source) -> SharedString {
    let file = |path: &'static str| {
        let name = std::path::Path::new(path).file_name();
        SharedString::from(name.and_then(|name| name.to_str()).unwrap_or(path))
    };
    match source {
        launch_defs::Source::File => file(launch_defs::LAUNCH_FILE),
        launch_defs::Source::LaunchJson => file(launch_defs::LAUNCH_JSON),
        launch_defs::Source::Detected => tr!("workbench-debug-picker-detected"),
    }
}

/// Why `config` can't be debugged, if it can't.
pub(crate) fn blocked(config: &LaunchConfig) -> Option<SharedString> {
    (!config.can_start()).then(|| {
        tr!(
            "workbench-debug-run-blocked",
            why = config.blockers.join("; ")
        )
    })
}

/// What the tests read of the widget's debugging controls as the last
/// frame drew them.
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Drawn {
    /// Each control: its id, accessible name, whether it's enabled and the
    /// key its tooltip names.
    pub controls: Vec<(&'static str, SharedString, bool, Option<String>)>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static DRAWN: std::cell::RefCell<Drawn> = std::cell::RefCell::default();
    /// The last command a control of the widget's dispatched, and its args.
    pub(crate) static DISPATCHED: std::cell::RefCell<Vec<(String, Value)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Notes a control the widget draws, for the tests.
fn drew(id: &'static str, name: &SharedString, enabled: bool, key: Option<&String>) {
    #[cfg(test)]
    {
        crate::title_bar::DRAWN.with_borrow_mut(|drawn| drawn.push((id, name.clone())));
        DRAWN.with_borrow_mut(|drawn| {
            (drawn.controls).push((id, name.clone(), enabled, key.cloned()));
        });
    }
    let _ = (id, name, enabled, key);
}

/// Clears what the tests read, as the title bar draws afresh.
pub(crate) fn drawing() {
    #[cfg(test)]
    DRAWN.with_borrow_mut(|drawn| drawn.controls.clear());
}

/// Runs workbench command `command` with `args` from a click.
fn dispatch(command: &'static str, args: Value, window: &mut Window, cx: &mut App) {
    #[cfg(test)]
    DISPATCHED.with_borrow_mut(|sent| sent.push((command.to_owned(), args.clone())));
    let Ok(invocation) = Invocation::of(command) else {
        return;
    };
    let context = [ContextFrame::new(crate::KEY_CONTEXT)]
        .into_iter()
        .collect();
    let invocation = (invocation.args(args).source(Source::Mouse)).context(context);
    window.dispatch_action(Box::new(RunCommand(invocation)), cx);
}

/// A tooltip of `title` and `key`.
fn tooltip(
    title: SharedString,
    key: Option<String>,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| {
        let tooltip = Tooltip::new(title.clone(), key.clone(), &fe_ui::ui_theme(cx));
        #[cfg(test)]
        crate::title_bar::LAST_TOOLTIP.with_borrow_mut(|last| *last = Some(tooltip.text()));
        cx.new(|_| tooltip).into()
    }
}

/// `debug.start`'s key, which shows its picker (or continues a stopped
/// session).
fn start_key(cx: &App) -> Option<String> {
    debug_toolbar::key("debug.start", DEBUG_SESSION, cx)
}

/// 🐞 Debug… with a task chosen: `debug.start`'s picker, at the
/// configurations.
pub(crate) fn debug_pick_button(cx: &App) -> AnyElement {
    let name = tr!("workbench-debug-run-debug-pick");
    let key = start_key(cx);
    drew(DEBUG_PICK_ID, &name, true, key.as_ref());
    let mut button = IconButton::new(DEBUG_PICK_ID, Icon::Bug, name.clone(), &fe_ui::ui_theme(cx))
        .tooltip(tooltip(name, key.clone()))
        .on_click(|_, window, cx| dispatch("debug.start", Value::Null, window, cx));
    if let Some(key) = key {
        button = button.shortcut(key);
    }
    button.into_any_element()
}

/// 🐞 Debug with `config` chosen: `debug.start` of it, or, if it can't
/// start, disabled with why as its tooltip.
pub(crate) fn debug_button(config: &LaunchConfig, cx: &App) -> AnyElement {
    let name = tr!("workbench-debug-run-debug");
    let why = blocked(config);
    drew(DEBUG_ID, &name, why.is_none(), None);
    let button = IconButton::new(DEBUG_ID, Icon::Bug, name, &fe_ui::ui_theme(cx));
    match why {
        Some(why) => button.disabled(true).tooltip(tooltip(why, None)),
        None => {
            let button = button.tooltip(tooltip(tr!("workbench-debug-run-debug"), None));
            let args = Value::from_iter([(
                verbatim!("configuration"),
                Value::from(config.name.as_str()),
            )]);
            button.on_click(move |_, window, cx| dispatch("debug.start", args.clone(), window, cx))
        }
    }
    .into_any_element()
}

/// The `bug` icon a configuration's label carries.
pub(crate) fn label_bug(cx: &App) -> AnyElement {
    let color = cx.theme().colors.list.icon;
    svg()
        .path(Icon::Bug.path())
        .size(px(14.))
        .flex_none()
        .text_color(color)
        .into_any_element()
}

/// A configuration's label's accessible name.
pub(crate) fn config_label_name(name: &str) -> SharedString {
    tr!("workbench-debug-run-config", name = name)
}

/// The session's look, to draw the widget again only when it changes:
/// its label's name and state icon, and each button's id and whether
/// it's enabled. `None` with no session.
pub(crate) fn session_look(debugger: &Debugger) -> Option<SessionLook> {
    if !debugger.is_active() {
        return None;
    }
    let name = session_name(debugger);
    let (specs, _) = debug_toolbar::specs(debugger);
    let buttons = (specs.iter())
        .map(|spec| (title_id(spec.id), spec.control.is_enabled()))
        .collect();
    Some((
        debug_toolbar::label_name(debugger, &name),
        stopped(debugger),
        buttons,
    ))
}

/// The session's configuration.
fn session_name(debugger: &Debugger) -> String {
    (debugger.starting())
        .or_else(|| debugger.configuration())
        .unwrap_or_default()
        .to_owned()
}

fn stopped(debugger: &Debugger) -> bool {
    debugger.starting().is_none() && debugger.state() == Some(fe_dap::SessionState::Stopped)
}

/// The title bar's id for the toolbar's button `id`.
fn title_id(id: &'static str) -> &'static str {
    match id {
        "debug-toolbar-continue" => "title-run-debug-continue",
        "debug-toolbar-pause" => "title-run-debug-pause",
        "debug-toolbar-step-over" => "title-run-debug-step-over",
        "debug-toolbar-step-into" => "title-run-debug-step-into",
        "debug-toolbar-step-out" => "title-run-debug-step-out",
        "debug-toolbar-restart" => "title-run-debug-restart",
        "debug-toolbar-stop" => "title-run-debug-stop",
        other => other,
    }
}

/// The session's controls: its label, which `on_label` opens the session
/// menu from, then its buttons.
pub(crate) fn session_element(
    debugger: &Debugger,
    on_label: impl Fn(&mut Window, &mut App) + 'static,
    label_max: gpui::Pixels,
    cx: &App,
) -> AnyElement {
    let name = session_name(debugger);
    let label_name = debug_toolbar::label_name(debugger, &name);
    drew(SESSION_ID, &label_name, true, None);
    let icon = match stopped(debugger) {
        true => Icon::Pause,
        false => Icon::LoaderCircle,
    };
    let label = Button::new(SESSION_ID, Text::Verbatim(name.into()))
        .ghost()
        .icon(icon)
        .name(label_name)
        .flex_row_reverse()
        .flex_shrink(1.)
        .min_w_0()
        .max_w(label_max)
        .debug_selector(|| SESSION_ID.into())
        .tooltip(tooltip(tr!("workbench-debug-run-session-menu"), None))
        .on_click(move |_, window, cx| on_label(window, cx));
    let (specs, gap) = debug_toolbar::specs(debugger);
    let buttons = specs.into_iter().enumerate().map(|(ix, spec)| {
        let id = title_id(spec.id);
        let command = spec.command;
        let enabled = spec.control.is_enabled();
        let key = debug_toolbar::key(spec.key, spec.flag, cx);
        drew(id, &spec.name, enabled, key.as_ref());
        let mut button = IconButton::new(id, spec.icon, spec.name.clone(), &fe_ui::ui_theme(cx))
            .tooltip(tooltip(spec.name, key.clone()))
            .disabled(!enabled)
            .on_click(move |_, window, cx| dispatch(command, Value::Null, window, cx));
        if let Some(key) = key {
            button = button.shortcut(key);
        }
        let space = if ix == gap { GROUP_GAP - GAP } else { px(0.) };
        div().flex_none().ml(space).child(button)
    });
    div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(GAP)
        .h(height::CONTROL_SMALL)
        .child(label)
        .children(buttons.collect::<Vec<_>>())
        .into_any_element()
}

/// What a pick in the run picker hands back.
pub(crate) type OnChoose = Rc<dyn Fn(RunChoice, &mut Window, &mut App)>;

/// What the session menu's Choose Task or Configuration… opens.
pub(crate) type OnPick = Rc<dyn Fn(&mut Window, &mut App)>;

/// A session's look: its label's name, whether it's stopped, and each
/// button's id and whether it's enabled.
pub(crate) type SessionLook = (SharedString, bool, Vec<(&'static str, bool)>);

/// A row of the run picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// The tasks' heading, then the configurations'.
    TasksHeading,
    ConfigsHeading,
    Task(usize),
    Config(usize),
}

/// A configuration as the picker lists it.
struct ConfigRow {
    name: String,
    source: SharedString,
    blocked: Option<SharedString>,
}

/// The picker over a root's tasks and launch configurations: confirming
/// one chooses it.
pub(crate) struct RunPicker {
    labels: Vec<String>,
    configs: Vec<ConfigRow>,
    rows: Vec<Row>,
    choose: OnChoose,
    /// What was focused when it opened.
    previous_focus: Option<FocusHandle>,
}

impl RunPicker {
    fn refilter(&mut self, query: &str) {
        let query = query.to_lowercase();
        let tasks = (self.labels.iter().enumerate())
            .filter(|(_, label)| label.to_lowercase().contains(&query))
            .map(|(ix, _)| Row::Task(ix));
        let configs = (self.configs.iter().enumerate())
            .filter(|(_, config)| {
                config.name.to_lowercase().contains(&query)
                    || config.source.to_lowercase().contains(&query)
            })
            .map(|(ix, _)| Row::Config(ix));
        let (tasks, configs): (Vec<Row>, Vec<Row>) = (tasks.collect(), configs.collect());
        // Headings only once there are two kinds to tell apart.
        let grouped = !self.configs.is_empty();
        self.rows.clear();
        if grouped && !tasks.is_empty() {
            self.rows.push(Row::TasksHeading);
        }
        self.rows.extend(tasks);
        if grouped && !configs.is_empty() {
            self.rows.push(Row::ConfigsHeading);
        }
        self.rows.extend(configs);
    }

    /// The first row that picks something.
    fn first_pick(&self) -> usize {
        let heading = |row: &Row| matches!(row, Row::TasksHeading | Row::ConfigsHeading);
        self.rows.iter().position(|row| !heading(row)).unwrap_or(0)
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    /// The rows as the tests read them: a heading's text, a task's label,
    /// or a configuration's name and source.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> Vec<String> {
        (0..self.rows.len())
            .map(|ix| match self.rows[ix] {
                Row::TasksHeading => format!("# {}", tr!("workbench-debug-run-tasks")),
                Row::ConfigsHeading => format!("# {}", tr!("workbench-debug-run-configs")),
                Row::Task(task) => self.labels[task].clone(),
                Row::Config(config) => {
                    let config = &self.configs[config];
                    format!("{} · {}", config.name, config.source)
                }
            })
            .collect()
    }
}

impl PickerDelegate for RunPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.refilter(&query);
        cx.notify();
        let picker = cx.entity();
        let first = self.first_pick();
        // `Picker::select` borrows the picker, which is being updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.select(first, cx)));
    }

    fn match_count(&self) -> usize {
        self.rows.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        match self.rows[ix] {
            Row::TasksHeading => SharedString::new_static(verbatim!("run-tasks-heading")),
            Row::ConfigsHeading => SharedString::new_static(verbatim!("run-configs-heading")),
            Row::Task(task) => format!("run-task-row-{task}").into(),
            Row::Config(config) => format!("run-config-row-{config}").into(),
        }
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let line = || {
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap_2()
                .overflow_hidden()
                .whitespace_nowrap()
        };
        let heading = |text: SharedString| {
            line()
                .text_size(fe_ui::UI_SMALL.size)
                .text_color(ui.muted_text)
                .child(text)
                .into_any_element()
        };
        match self.rows[ix] {
            Row::TasksHeading => heading(tr!("workbench-debug-run-tasks")),
            Row::ConfigsHeading => heading(tr!("workbench-debug-run-configs")),
            Row::Task(task) => div()
                .truncate()
                .child(self.labels[task].clone())
                .into_any_element(),
            Row::Config(config) => {
                let config = &self.configs[config];
                let line = match config.blocked {
                    None => line(),
                    Some(_) => line().text_color(ui.muted_text),
                };
                line.child(label_bug(cx))
                    .child(div().flex_none().child(config.name.clone()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(ui.muted_text)
                            .child(config.source.clone()),
                    )
                    .into_any_element()
            }
        }
    }

    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        match self.rows[ix] {
            Row::TasksHeading => tr!("workbench-debug-run-tasks"),
            Row::ConfigsHeading => tr!("workbench-debug-run-configs"),
            Row::Task(task) => self.labels[task].clone().into(),
            Row::Config(config) => {
                let config = &self.configs[config];
                let blocked = config.blocked.as_deref().unwrap_or_default();
                joined_label([config.name.as_str(), &config.source, blocked])
            }
        }
    }

    /// A task or a configuration is chosen, focus going back where it was
    /// first; a heading picks nothing.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let choice = match self.rows[ix] {
            Row::TasksHeading | Row::ConfigsHeading => return false,
            Row::Task(task) => RunChoice::Task(self.labels[task].clone()),
            Row::Config(config) => RunChoice::Debug(self.configs[config].name.clone()),
        };
        self.restore_focus(window, cx);
        (self.choose)(choice, window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }
}

/// The run picker, while it shows.
pub(crate) type RunPickerHandle = WeakEntity<Picker<RunPicker>>;

/// The picker's rows for `configs`.
fn config_rows(configs: &[LaunchConfig]) -> Vec<ConfigRow> {
    (configs.iter())
        .map(|config| ConfigRow {
            name: config.name.clone(),
            source: source_name(config.source),
            blocked: blocked(config),
        })
        .collect()
}

/// Hands the picker configurations read since it opened, keeping its query
/// and, where it can, its selection. Whether the picker is still there.
pub(crate) fn update_run_picker(
    picker: &RunPickerHandle,
    configs: &[LaunchConfig],
    cx: &mut App,
) -> bool {
    let updated = picker.update(cx, |picker, cx| {
        picker.delegate.configs = config_rows(configs);
        let query = picker.input().read(cx).text().to_owned();
        picker.delegate.refilter(&query);
        picker.matches_updated(false, cx);
    });
    updated.is_ok()
}

/// Shows the run picker over `tasks` and `configs`, `choose` taking what's
/// picked.
pub(crate) fn show_run_picker(
    workbench: &Entity<Workbench>,
    tasks: &[Task],
    configs: &[LaunchConfig],
    choose: OnChoose,
    previous_focus: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> RunPickerHandle {
    let delegate = RunPicker {
        labels: tasks.iter().map(|task| task.label.clone()).collect(),
        configs: config_rows(configs),
        rows: Vec::new(),
        choose,
        previous_focus,
    };
    let placeholder = match configs.is_empty() {
        true => text!("workbench-run-choose-placeholder"),
        false => text!("workbench-debug-run-choose-placeholder"),
    };
    let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
    picker.update(cx, |picker, cx| {
        picker.delegate.refilter("");
        let first = picker.delegate.first_pick();
        picker.select(first, cx);
        picker.show(window, cx);
    });
    let handle = picker.downgrade();
    let shell = workbench.read(cx).shell().clone();
    shell.update(cx, |shell, cx| {
        shell.show_modal(RUN_PICKER, picker, window, cx)
    });
    handle
}

/// An item of the session menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuItem {
    ShowDebugger,
    ShowLog,
    Choose,
}

impl MenuItem {
    const ALL: [Self; 3] = [Self::ShowDebugger, Self::ShowLog, Self::Choose];

    fn title(self) -> SharedString {
        match self {
            Self::ShowDebugger => tr!("workbench-debug-run-show-debugger"),
            Self::ShowLog => tr!("workbench-debug-run-show-log"),
            Self::Choose => tr!("workbench-debug-run-choose"),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::ShowDebugger => "run-session-show-debugger",
            Self::ShowLog => "run-session-show-log",
            Self::Choose => "run-session-choose",
        }
    }
}

/// The session menu (D0): Show Debugger, Show Adapter Log, and Choose Task
/// or Configuration…, which `choose` opens.
pub(crate) struct SessionMenu {
    workbench: WeakEntity<Workbench>,
    items: Vec<MenuItem>,
    choose: OnPick,
    previous_focus: Option<FocusHandle>,
}

impl SessionMenu {
    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    /// Its items' titles, for the tests.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> Vec<SharedString> {
        self.items.iter().map(|item| item.title()).collect()
    }
}

impl PickerDelegate for SessionMenu {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.items = (MenuItem::ALL.into_iter())
            .filter(|item| item.title().to_lowercase().contains(&query))
            .collect();
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.items.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        SharedString::new_static(self.items[ix].id())
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        div()
            .truncate()
            .child(self.items[ix].title())
            .into_any_element()
    }

    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        self.items[ix].title()
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let item = self.items[ix];
        self.restore_focus(window, cx);
        match item {
            MenuItem::ShowDebugger => {
                // `debug.show_panels` toggles: shown already, it stays.
                let shown = (self.workbench.upgrade()).is_some_and(|workbench| {
                    let panel = PanelId(crate::debug_panel::PANEL);
                    workbench.read(cx).tool_shown(panel, cx)
                });
                if !shown {
                    dispatch(crate::debug_panel::SHOW_COMMAND, Value::Null, window, cx);
                }
            }
            MenuItem::ShowLog => dispatch("debug.show_log", Value::Null, window, cx),
            MenuItem::Choose => {
                // After this menu is gone from the modal layer.
                let choose = self.choose.clone();
                window.defer(cx, move |window, cx| choose(window, cx));
            }
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn title(&self, _: &App) -> Option<SharedString> {
        Some(tr!("workbench-debug-run-session-menu"))
    }
}

/// Shows the session menu, `choose` opening the run picker from it.
pub(crate) fn show_session_menu(
    workbench: &Entity<Workbench>,
    choose: OnPick,
    window: &mut Window,
    cx: &mut App,
) {
    let delegate = SessionMenu {
        workbench: workbench.downgrade(),
        items: MenuItem::ALL.to_vec(),
        choose,
        previous_focus: window.focused(cx),
    };
    let placeholder = text!("workbench-debug-run-session-placeholder");
    let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
    picker.update(cx, |picker, cx| picker.show(window, cx));
    let shell = workbench.read(cx).shell().clone();
    shell.update(cx, |shell, cx| {
        shell.show_modal(SESSION_MENU, picker, window, cx)
    });
}

#[cfg(test)]
impl Workbench {
    /// The run widget's picker's rows, if it's the modal showing.
    pub(crate) fn run_picker_rows(&self, cx: &App) -> Option<Vec<String>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == RUN_PICKER)?;
        let picker: Entity<Picker<RunPicker>> = layer.modal_view()?.clone().downcast().ok()?;
        Some(picker.read(cx).delegate.shown())
    }

    /// The session menu's items, if it's the modal showing.
    pub(crate) fn session_menu_items(&self, cx: &App) -> Option<Vec<SharedString>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == SESSION_MENU)?;
        let picker: Entity<Picker<SessionMenu>> = layer.modal_view()?.clone().downcast().ok()?;
        Some(picker.read(cx).delegate.shown())
    }
}
