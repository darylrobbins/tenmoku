//! The Runs tool window of the bottom panel (`docs/design/design-system.md`,
//! "Runs: builds, tests and agent tasks in one list"): the workbench's
//! [`Runs`], newest first, one 24 px row each. A row has a status icon (a
//! spinner while it runs, a check in `status.success` once it passed, a
//! cross in `status.error` once it failed, a muted dash once cancelled), the
//! run's title in code small, a one-line summary in UI small, muted, and its
//! age at the right (`now`, `12s`, `3m`, `2h`, `4d`: [`age_label`]). A
//! running row draws a 2 px accent line along its bottom edge, filled to how
//! far along it is when that's known, else the whole line at
//! [`UNKNOWN_PROGRESS_ALPHA`] ([`progress_line`]).
//!
//! The ages are redrawn once a second while the panel draws and a row is
//! under a minute old; the panel draws only while the bottom panel shows it.
//! A running row's spinner turns once a second, however old the row: it
//! asks for the next frame each time it draws, so it turns while it's drawn
//! and stops once the run ends or the panel hides. With reduced motion
//! (`App::reduce_motion`) it stands still.
//!
//! With vim on, the dock keys leave it as they leave Problems (`escape`,
//! `<C-w>h`/`j`/`k`/`l`/`p`/`w` and `<C-h>`/`<C-j>`/`<C-k>`/`<C-l>`): its key
//! context is [`KEY_CONTEXT`], with the `vim` flag.
//!
//! A click on a row, or Enter on the selected one, shows the terminal tab of
//! the task that made the run, if it's still open ([`Workbench::open_run`]).
//! The list is kiln-ui's [`List`], so the arrow keys move the selection, and
//! each row is a list option named "<title>, <status>, <age>" to a screen
//! reader ([`Row::name`]). With no runs, a centred empty state names the
//! command that runs a task.
//!
//! `runs.show` (`workbench_commands.rs`) shows it; the dock registers it
//! ([`crate::dock`]) and its rail button, at the rail's foot after
//! Problems'. Its text is the catalogue's (`l10n/en-US/runs.ftl`).

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use fe_ui::{
    ActiveTheme as _, CODE_SMALL, FamilySlot, FontFamilies, Icon, List, ListDelegate, ListEvent,
    ListRow, RowState, Tokens, UI_HEADING, UI_SMALL,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, Pixels, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Transformation,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, radians, relative, svg,
};
use kiln_l10n::tr;

use crate::{
    Workbench,
    runs::{Run, RunId, RunStatus, RunSummary, Runs},
};

/// The tool window's id in the bottom dock, and its rail button's.
pub(crate) const PANEL: &str = "runs";

/// The command that shows it.
pub(crate) const SHOW_COMMAND: &str = "runs.show";

/// Its rail button's and tab's icon (T1516): a play triangle with list
/// lines.
pub(crate) const ICON: Icon = Icon::ListVideo;

/// Its key context, which the keymap's dock keys name, with
/// [`VIM_FLAG`] while vim is on.
pub(crate) const KEY_CONTEXT: &str = "runs_panel";
const VIM_FLAG: &str = "vim";

/// The command the empty state's hint names: what starts a run.
const START_COMMAND: &str = "tasks.run";

/// A running row's line along its bottom edge.
pub(crate) const PROGRESS_HEIGHT: Pixels = px(2.);

/// The alpha of a running row's line while how far along it is isn't
/// known, drawn the whole width.
pub(crate) const UNKNOWN_PROGRESS_ALPHA: f32 = 0.4;

/// How long the spinner takes to turn once.
const SPIN: Duration = Duration::from_millis(1000);

/// The spinner's angle `since` its epoch, in radians: one turn a [`SPIN`].
pub(crate) fn spin_angle(since: Duration) -> f32 {
    let turn = (since.as_secs_f32() % SPIN.as_secs_f32()) / SPIN.as_secs_f32();
    turn * std::f32::consts::TAU
}

/// How often the ages are redrawn while one is under a minute.
const TICK: Duration = Duration::from_secs(1);

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// Under this a run's age reads `now`.
const NOW: u64 = 5;

/// The test selectors: the empty state, and a row and its progress line,
/// with the row's index after a colon.
pub(crate) const EMPTY_SELECTOR: &str = "runs-empty";
pub(crate) const ROW_SELECTOR: &str = "runs-row";
pub(crate) const PROGRESS_SELECTOR: &str = "runs-progress";

/// Row `ix`'s test selector.
pub(crate) fn row_selector(ix: usize) -> String {
    format!("{ROW_SELECTOR}:{ix}")
}

/// Row `ix`'s progress line's test selector.
pub(crate) fn progress_selector(ix: usize) -> String {
    format!("{PROGRESS_SELECTOR}:{ix}")
}

/// A run's status icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusIcon {
    Spinner,
    Passed,
    Failed,
    Cancelled,
}

impl StatusIcon {
    pub(crate) fn of(status: RunStatus) -> Self {
        match status {
            RunStatus::Running { .. } => Self::Spinner,
            RunStatus::Passed => Self::Passed,
            RunStatus::Failed { .. } => Self::Failed,
            RunStatus::Cancelled => Self::Cancelled,
        }
    }

    pub(crate) fn icon(self) -> Icon {
        match self {
            Self::Spinner => Icon::LoaderCircle,
            Self::Passed => Icon::Check,
            Self::Failed => Icon::X,
            Self::Cancelled => Icon::Minus,
        }
    }

    /// Its colour in `c`: celadon for passed, shinsha for failed, muted for
    /// cancelled.
    pub(crate) fn colour(self, c: &Tokens<Hsla>) -> Hsla {
        match self {
            Self::Spinner => c.text.primary,
            Self::Passed => c.status.success,
            Self::Failed => c.status.error,
            Self::Cancelled => c.text.muted,
        }
    }
}

/// The line a row with `status` draws along its bottom edge, if it's
/// running: the fraction of the row's width it fills, and its colour,
/// `accent` filled to the progress when it's known, else the whole width at
/// [`UNKNOWN_PROGRESS_ALPHA`].
pub(crate) fn progress_line(status: RunStatus, accent: Hsla) -> Option<(f32, Hsla)> {
    match status {
        RunStatus::Running {
            progress: Some(progress),
        } => Some((progress.clamp(0., 1.), accent)),
        RunStatus::Running { progress: None } => Some((1., accent.opacity(UNKNOWN_PROGRESS_ALPHA))),
        _ => None,
    }
}

/// `age` as a row draws it: `now` under [`NOW`] seconds, then whole
/// seconds, minutes, hours and days, each rounded down.
pub(crate) fn age_label(age: Duration) -> SharedString {
    match age.as_secs() {
        secs if secs < NOW => tr!("workbench-runs-age-now"),
        secs if secs < MINUTE => tr!("workbench-runs-age-seconds", count = secs),
        secs if secs < HOUR => tr!("workbench-runs-age-minutes", count = secs / MINUTE),
        secs if secs < DAY => tr!("workbench-runs-age-hours", count = secs / HOUR),
        secs => tr!("workbench-runs-age-days", count = secs / DAY),
    }
}

/// `age` spelled out for a screen reader, in [`age_label`]'s buckets.
pub(crate) fn age_name(age: Duration) -> SharedString {
    match age.as_secs() {
        secs if secs < NOW => tr!("workbench-runs-age-name-now"),
        secs if secs < MINUTE => tr!("workbench-runs-age-name-seconds", count = secs),
        secs if secs < HOUR => tr!("workbench-runs-age-name-minutes", count = secs / MINUTE),
        secs if secs < DAY => tr!("workbench-runs-age-name-hours", count = secs / HOUR),
        secs => tr!("workbench-runs-age-name-days", count = secs / DAY),
    }
}

/// `status` in words, for a row's accessible name.
pub(crate) fn status_name(status: RunStatus) -> SharedString {
    match status {
        RunStatus::Running {
            progress: Some(progress),
        } => {
            let percent = (progress.clamp(0., 1.) * 100.).round() as u32;
            tr!("workbench-runs-status-running-progress", percent = percent)
        }
        RunStatus::Running { progress: None } => tr!("workbench-runs-status-running"),
        RunStatus::Passed => tr!("workbench-runs-status-passed"),
        RunStatus::Failed { .. } => tr!("workbench-runs-status-failed"),
        RunStatus::Cancelled => tr!("workbench-runs-status-cancelled"),
    }
}

/// A run's one line: what its summary says, else what its status does.
pub(crate) fn summary_text(status: RunStatus, summary: RunSummary) -> SharedString {
    match summary {
        RunSummary::None => match status {
            RunStatus::Running { .. } => tr!("workbench-runs-summary-running"),
            RunStatus::Passed => tr!("workbench-runs-summary-passed"),
            RunStatus::Failed { code: Some(code) } => {
                tr!("workbench-runs-summary-exit", code = code)
            }
            RunStatus::Failed { code: None } => tr!("workbench-runs-summary-no-code"),
            RunStatus::Cancelled => tr!("workbench-runs-summary-cancelled"),
        },
        RunSummary::Exit(code) => tr!("workbench-runs-summary-exit", code = code),
        RunSummary::Tests {
            passed,
            failed,
            total: None,
        } => tr!(
            "workbench-runs-summary-tests",
            passed = passed,
            failed = failed
        ),
        RunSummary::Tests {
            passed,
            failed,
            total: Some(total),
        } => tr!(
            "workbench-runs-summary-tests-total",
            passed = passed,
            failed = failed,
            total = total
        ),
        RunSummary::Build {
            crates,
            warnings,
            errors,
        } => tr!(
            "workbench-runs-summary-build",
            crates = crates,
            warnings = warnings,
            errors = errors
        ),
    }
}

/// One run, as its row shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) id: RunId,
    pub(crate) title: SharedString,
    pub(crate) status: RunStatus,
    pub(crate) summary: SharedString,
    /// What its age counts from: when it ended, else when it began.
    since: Instant,
}

impl Row {
    fn of(run: &Run) -> Self {
        Self {
            id: run.id,
            title: run.title.clone().into(),
            status: run.status,
            summary: summary_text(run.status, run.summary),
            since: run.finished.unwrap_or(run.started),
        }
    }

    pub(crate) fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.since)
    }

    /// Its accessible name: "<title>, <status>, <age>".
    pub(crate) fn name(&self, now: Instant) -> SharedString {
        tr!(
            "workbench-runs-row-name",
            title = &self.title,
            status = status_name(self.status),
            age = age_name(self.age(now))
        )
    }
}

/// What opens a run's task tab: [`Workbench::open_run`], once the event
/// that asked is over.
type Opener = Rc<dyn Fn(RunId, &mut Window, &mut App)>;

/// The list's rows, newest first.
pub(crate) struct RunRows {
    rows: Vec<Row>,
    open: Opener,
    /// What the spinners' angle counts from, so all turn together.
    epoch: Instant,
    /// Each drawn row's progress line, by row: the fraction it fills and
    /// its colour, for tests.
    #[cfg(test)]
    pub(crate) drawn_lines: std::collections::BTreeMap<usize, (f32, Hsla)>,
    /// Each drawn spinner's angle, by row, for tests.
    #[cfg(test)]
    pub(crate) drawn_spins: std::collections::BTreeMap<usize, f32>,
    /// How many frames the spinners have asked for, for tests.
    #[cfg(test)]
    pub(crate) frames_asked: std::cell::Cell<usize>,
}

impl RunRows {
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Makes every row `by` older, for tests of old rows.
    #[cfg(test)]
    pub(crate) fn age_by(&mut self, by: Duration) {
        for row in &mut self.rows {
            row.since -= by;
        }
    }

    /// The spinner of a running row: its icon turning a [`SPIN`] at a time
    /// while it's drawn, a frame asked for each time it draws, unless the
    /// system asks for reduced motion, when it stands still.
    fn spinner(&self, window: &Window, cx: &App) -> Option<f32> {
        if cx.reduce_motion() {
            return None;
        }
        window.request_animation_frame();
        #[cfg(test)]
        self.frames_asked.set(self.frames_asked.get() + 1);
        Some(spin_angle(self.epoch.elapsed()))
    }
}

impl ListDelegate for RunRows {
    fn count(&self) -> usize {
        self.rows.len()
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        window: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let row = &self.rows[ix];
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let icon = StatusIcon::of(row.status);
        let line = progress_line(row.status, c.accent.base);
        let spin = match icon {
            StatusIcon::Spinner => self.spinner(window, cx),
            _ => None,
        };
        #[cfg(test)]
        match spin {
            Some(angle) => self.drawn_spins.insert(ix, angle),
            None => self.drawn_spins.remove(&ix),
        };
        let row = &self.rows[ix];
        #[cfg(test)]
        match line {
            Some(line) => self.drawn_lines.insert(ix, line),
            None => self.drawn_lines.remove(&ix),
        };
        let (id, open) = (row.id, self.open.clone());
        let body = ListRow::new(state)
            .relative()
            .gap_2()
            .px_3()
            .child(
                svg()
                    .path(icon.icon().path())
                    .size(px(14.))
                    .flex_none()
                    .when_some(spin, |icon, angle| {
                        icon.with_transformation(Transformation::rotate(radians(angle)))
                    })
                    .text_color(icon.colour(&c)),
            )
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .max_w(relative(0.5))
                    .truncate()
                    .font_family(code)
                    .text_size(CODE_SMALL.size)
                    .line_height(CODE_SMALL.line_height)
                    .text_color(c.text.primary)
                    .child(row.title.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(UI_SMALL.size)
                    .line_height(UI_SMALL.line_height)
                    .text_color(c.text.muted)
                    .child(row.summary.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(UI_SMALL.size)
                    .line_height(UI_SMALL.line_height)
                    .text_color(c.text.muted)
                    .child(age_label(row.age(Instant::now()))),
            )
            .when_some(line, |body, (fill, colour)| {
                body.child(
                    div()
                        .debug_selector(move || progress_selector(ix))
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .h(PROGRESS_HEIGHT)
                        .w(relative(fill))
                        .bg(colour),
                )
            });
        // The list selects the row; the click opens it too.
        div()
            .id(("run-row", ix))
            .debug_selector(move || row_selector(ix))
            .w_full()
            .on_click(move |_, window, cx| open(id, window, cx))
            .child(body)
            .into_any_element()
    }

    fn accessible_name(&self, _: &App) -> SharedString {
        tr!("workbench-runs-title")
    }

    fn row_name(&self, ix: usize, _: &App) -> SharedString {
        self.rows[ix].name(Instant::now())
    }
}

/// The Runs tool window: see the module docs.
pub(crate) struct RunsPanel {
    runs: Entity<Runs>,
    list: Entity<List<RunRows>>,
    /// The redraw due in a [`TICK`], while a row is under a minute old.
    tick: Option<Task<()>>,
    _runs: Subscription,
    _list: Subscription,
}

impl RunsPanel {
    /// Lists `runs`, opening a run's task tab in `workbench`.
    pub(crate) fn new(
        runs: &Entity<Runs>,
        workbench: WeakEntity<Workbench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let open: Opener = Rc::new(move |run: RunId, window: &mut Window, cx: &mut App| {
            let workbench = workbench.clone();
            // Outside whatever update the click or key came in.
            window.defer(cx, move |window, cx| {
                let opened = workbench.update(cx, |workbench, cx| {
                    workbench.open_run(run, window, cx);
                });
                opened.ok();
            });
        });
        let rows = RunRows {
            rows: Vec::new(),
            open,
            epoch: Instant::now(),
            #[cfg(test)]
            drawn_lines: Default::default(),
            #[cfg(test)]
            drawn_spins: Default::default(),
            #[cfg(test)]
            frames_asked: Default::default(),
        };
        let list = cx.new(|cx| List::new(rows, cx));
        let confirmed = cx.subscribe_in(&list, window, |_, list, event, window, cx| {
            if let ListEvent::Confirmed(ix) = *event {
                let delegate = list.read(cx).delegate();
                if let Some(row) = delegate.rows.get(ix) {
                    let (id, open) = (row.id, delegate.open.clone());
                    open(id, window, cx);
                }
            }
        });
        let mut panel = Self {
            runs: runs.clone(),
            list,
            tick: None,
            _runs: cx.observe(runs, |panel, _, cx| panel.list_runs(cx)),
            _list: confirmed,
        };
        panel.list_runs(cx);
        panel
    }

    /// The list, for tests.
    #[cfg(test)]
    pub(crate) fn list(&self) -> &Entity<List<RunRows>> {
        &self.list
    }

    /// Lists the runs again, newest first, keeping the selected run
    /// selected (none if it's gone) without scrolling the list.
    fn list_runs(&mut self, cx: &mut Context<Self>) {
        let rows: Vec<Row> = self.runs.read(cx).list().map(Row::of).collect();
        self.list.update(cx, |list, cx| {
            let selected = (list.selected())
                .and_then(|ix| list.delegate().rows.get(ix))
                .map(|row| row.id);
            let before = list.selected();
            list.delegate_mut().rows = rows;
            let at =
                selected.and_then(|id| (list.delegate().rows.iter()).position(|row| row.id == id));
            list.rows_changed(cx);
            // Where it's moved to, if a row came above it, but left
            // wherever the list is scrolled: an update in the background
            // never scrolls.
            if selected.is_some() && at != before {
                list.keep_selected(at, cx);
            }
        });
        cx.notify();
    }

    /// Redraws the list in a [`TICK`] if a row is under a minute old and
    /// none is due yet.
    fn keep_ages_fresh(&mut self, cx: &mut Context<Self>) {
        if self.tick.is_some() {
            return;
        }
        let now = Instant::now();
        let rows = self.list.read(cx).delegate().rows();
        if !rows.iter().any(|row| row.age(now).as_secs() < MINUTE) {
            return;
        }
        self.tick = Some(cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(TICK).await;
            let ticked = panel.update(cx, |panel, cx| {
                panel.tick = None;
                panel.list.update(cx, |_, cx| cx.notify());
                cx.notify();
            });
            ticked.ok();
        }));
    }

    /// The empty state, centred: an icon, the heading and a hint naming the
    /// command that runs a task.
    fn render_empty(&self, cx: &App) -> gpui::Div {
        let c = cx.theme().colors;
        let command = fe_command::l10n::command_label(START_COMMAND, "");
        div()
            .debug_selector(|| EMPTY_SELECTOR.into())
            .track_focus(&self.list.focus_handle(cx))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .px_4()
            .text_center()
            .child(
                svg()
                    .path(ICON.path())
                    .size(px(24.))
                    .flex_none()
                    .text_color(c.text.muted),
            )
            .child(
                div()
                    .text_size(UI_HEADING.size)
                    .line_height(UI_HEADING.line_height)
                    .font_weight(gpui::FontWeight(UI_HEADING.weight as f32))
                    .text_color(c.text.primary)
                    .child(tr!("workbench-runs-empty")),
            )
            .child(
                div()
                    .text_color(c.text.muted)
                    .child(tr!("workbench-runs-empty-hint", command = command)),
            )
    }
}

impl Focusable for RunsPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.list.focus_handle(cx)
    }
}

impl Render for RunsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.keep_ages_fresh(cx);
        let ui = fe_ui::ui_theme(cx);
        let empty = self.list.read(cx).delegate().rows.is_empty();
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if fe_gpui::vim_enabled(cx) {
            context.add(VIM_FLAG);
        }
        let body = if empty {
            self.render_empty(cx).into_any_element()
        } else {
            self.list.clone().into_any_element()
        };
        div()
            .key_context(context)
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .child(body)
    }
}

impl Workbench {
    /// Shows the terminal tab of the task whose session made `run`, if the
    /// tab's still open, focused, in the bottom panel. False if there's
    /// none.
    pub(crate) fn open_run(
        &mut self,
        run: RunId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(view) = self.task_view_of_run(run, cx) else {
            return false;
        };
        let terminal = self.terminal().clone();
        let at = (terminal.read(cx).views()).position(|shown| shown.entity_id() == view);
        let Some(at) = at else {
            return false;
        };
        self.show_terminal_dock(window, cx);
        terminal.update(cx, |terminal, cx| terminal.activate(at, window, cx));
        true
    }

    /// The Runs tool window, for tests.
    #[cfg(test)]
    pub(crate) fn runs_panel(&self) -> &Entity<RunsPanel> {
        &self.runs_panel
    }
}
