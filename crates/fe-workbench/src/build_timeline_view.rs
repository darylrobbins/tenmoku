//! A build run's detail in the Runs tool window (`docs/design/design-system.md`,
//! "Runs: builds, tests and agent tasks in one list": "a crate waterfall
//! instead of a wall of log: fresh crates as thin lines, rebuilt ones as
//! bars, the crate you edited in rust; messages beside it; Timeline or
//! Output"). The panel shows it beside its list, or under it when the panel
//! is narrow, while a build run is selected ([`crate::runs_panel`]).
//!
//! Its title row has the run's title and summary, and a Timeline | Output
//! [`SegmentedControl`] (T1518's).
//!
//! - **Timeline**: a time axis along the top, ticks every 1, 5, 10 or 30 s
//!   as the build's length asks ([`tick_step`]), then one [`ROW_HEIGHT`] row
//!   per crate in the order they started, a kiln-ui [`List`], so the arrow
//!   keys move through it and each row is an option a screen reader names.
//!   A row has the crate's name in code small, its track, its warning and
//!   error counts (▲ in kiseto, ✕ in shinsha) when the build has messages,
//!   and how long it took, right-aligned. On the track a fresh crate is a
//!   [`FRESH_WIDTH`] line where cargo reported it, a rebuilt one a bar from
//!   its start to its end ([`Mark`]), both scaled to the build's length; a
//!   local crate's in the accent, the others' in `text.muted` at
//!   [`OTHER_ALPHA`] ([`Tone`]). Where everything goes is [`layout`], a
//!   pure function of the timeline and the track's width, tested without a
//!   window. A click on a crate's row, or Enter on it, shows the Problems
//!   panel with the first of the crate's messages selected
//!   ([`Workbench::show_crate_problems`]).
//! - **Output**: the build's stderr, cargo's `Compiling` lines and the
//!   diagnostics it rendered ([`Workbench::build_output`]), in code small,
//!   scrolling.
//!
//! A running build is live: the timeline is the build as far as it's got
//! ([`Workbench::build_shown`]), read again as its program's output comes,
//! and the crates being compiled grow to the time it's been running, the
//! track's scale with them, redrawn every [`LIVE_TICK`].
//!
//! Its text is the catalogue's (`l10n/en-US/build_runs.ftl`).

use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use fe_runlog::BuildTimeline;
use fe_ui::{
    ActiveTheme as _, CODE_SMALL, FamilySlot, FontFamilies, Icon, List, ListDelegate, ListEvent,
    ListRow, RowState, SegmentedControl, Tokens, UI_SMALL,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, UniformListScrollHandle, WeakEntity, Window, canvas, div,
    prelude::FluentBuilder as _, px, svg, uniform_list,
};
use kiln_l10n::{tr, verbatim};

use crate::{
    Workbench,
    build_runner::message_crate,
    runs::{RunId, RunKind, RunStatus, RunSummary},
    runs_panel::summary_text,
};

/// A crate's row, and the axis's.
pub(crate) const ROW_HEIGHT: Pixels = px(18.);

/// How wide a fresh crate's line is.
pub(crate) const FRESH_WIDTH: Pixels = px(2.);

/// How narrow a rebuilt crate's bar may be: wider than a fresh line, so a
/// quick rebuild doesn't read as fresh.
pub(crate) const MIN_BAR_WIDTH: Pixels = px(4.);

/// How tall a crate's mark is, and how far down its row it starts: in the
/// middle.
pub(crate) const MARK_HEIGHT: Pixels = px(8.);
pub(crate) const MARK_TOP: Pixels = px(5.);

/// The alpha of a crate's mark that isn't local, in `text.muted`.
pub(crate) const OTHER_ALPHA: f32 = 0.5;

/// The columns of a row and the axis, around the track.
pub(crate) const NAME_WIDTH: Pixels = px(120.);
pub(crate) const COUNTS_WIDTH: Pixels = px(56.);
pub(crate) const DURATION_WIDTH: Pixels = px(48.);

/// How far apart the axis's ticks may be at most, so it has no more than
/// this many.
const MAX_TICKS: f64 = 10.;

/// The ticks' steps, in seconds, the smallest that keeps under
/// [`MAX_TICKS`] taken.
const TICK_STEPS: [u64; 4] = [1, 5, 10, 30];

/// How far apart two labelled ticks are at least: on a narrow track only
/// every second, fifth, … tick is labelled.
pub(crate) const LABEL_GAP: Pixels = px(32.);

/// How tall a tick is, at the axis's foot.
pub(crate) const TICK_HEIGHT: Pixels = px(4.);

/// How often a running build's waterfall is redrawn, its crates being
/// compiled grown to the time.
pub(crate) const LIVE_TICK: Duration = Duration::from_millis(250);

/// The test selectors: the segmented control, the axis, a crate's row and
/// its mark, with the row's index after a colon, and the output.
pub(crate) const SWITCH_ID: &str = "build-run-view";
pub(crate) const AXIS_SELECTOR: &str = "build-run-axis";
pub(crate) const ROW_SELECTOR: &str = "build-run-row";
pub(crate) const MARK_SELECTOR: &str = "build-run-mark";
pub(crate) const OUTPUT_SELECTOR: &str = "build-run-output";

/// Row `ix`'s test selector.
pub(crate) fn row_selector(ix: usize) -> String {
    format!("{ROW_SELECTOR}:{ix}")
}

/// Row `ix`'s mark's test selector.
pub(crate) fn mark_selector(ix: usize) -> String {
    format!("{MARK_SELECTOR}:{ix}")
}

/// Which of its two views the detail shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Timeline,
    Output,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Timeline, Tab::Output];

    fn label(self) -> SharedString {
        match self {
            Tab::Timeline => tr!("workbench-build-runs-timeline"),
            Tab::Output => tr!("workbench-build-runs-output"),
        }
    }
}

/// How a crate's row marks it on the track, from the track's left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mark {
    /// A fresh crate: a [`FRESH_WIDTH`] line at `x`.
    Line { x: Pixels },
    /// A rebuilt one: a bar from `x`, `width` wide, at least
    /// [`MIN_BAR_WIDTH`].
    Bar { x: Pixels, width: Pixels },
}

impl Mark {
    pub(crate) fn x(self) -> Pixels {
        match self {
            Mark::Line { x } | Mark::Bar { x, .. } => x,
        }
    }

    pub(crate) fn width(self) -> Pixels {
        match self {
            Mark::Line { .. } => FRESH_WIDTH,
            Mark::Bar { width, .. } => width,
        }
    }
}

/// Which colour a crate's mark is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// A path package: the crate being edited, in the accent.
    Local,
    /// One from a registry or git, in `text.muted` at [`OTHER_ALPHA`].
    Other,
}

impl Tone {
    pub(crate) fn colour(self, c: &Tokens<Hsla>) -> Hsla {
        match self {
            Tone::Local => c.accent.base,
            Tone::Other => c.text.muted.opacity(OTHER_ALPHA),
        }
    }
}

/// Where one crate goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CrateLayout {
    /// Its index in the timeline's spans.
    pub(crate) span: usize,
    pub(crate) mark: Mark,
    pub(crate) tone: Tone,
    /// How long it took, or has taken so far: none if it was fresh.
    pub(crate) duration: Option<Duration>,
    /// It's being compiled.
    pub(crate) running: bool,
}

/// A tick of the axis: `at` from the build's start, `x` from the track's
/// left edge, and whether it's labelled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tick {
    pub(crate) x: Pixels,
    pub(crate) at: Duration,
    pub(crate) labelled: bool,
}

/// Where everything in the waterfall goes: see [`layout`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    /// The build's length, which the track's width stands for.
    pub(crate) length: Duration,
    /// The ticks' step ([`tick_step`]) and the ticks, from 0 to the length.
    pub(crate) step: Duration,
    pub(crate) ticks: Vec<Tick>,
    /// The crates, in the order they started.
    pub(crate) rows: Vec<CrateLayout>,
}

/// The axis's step for a build `length` long: 1, 5, 10 or 30 s, the
/// smallest of them that puts no more than [`MAX_TICKS`] steps in it.
pub(crate) fn tick_step(length: Duration) -> Duration {
    let secs = length.as_secs_f64();
    let step = (TICK_STEPS.into_iter())
        .find(|&step| secs / step as f64 <= MAX_TICKS)
        .unwrap_or(TICK_STEPS[TICK_STEPS.len() - 1]);
    Duration::from_secs(step)
}

/// Where everything in the waterfall of `timeline` goes on a track `width`
/// wide. `elapsed` is how long a running build has been running: its
/// crates being compiled end there, and the build's at least that long.
/// The track stands for the build's length, from its start to the last
/// crate's end (a second if that's nothing); each crate's mark is placed
/// and sized by its span in proportion, kept on the track. The crates are
/// in the order they started, those that started together as cargo first
/// named them.
pub(crate) fn layout(timeline: &BuildTimeline, elapsed: Option<Duration>, width: Pixels) -> Layout {
    let end = |span: &fe_runlog::CrateSpan| match (span.running, elapsed) {
        (true, Some(elapsed)) => span.end.max(elapsed),
        _ => span.end,
    };
    let last = (timeline.spans.iter()).map(end).max().unwrap_or_default();
    let mut length = last.max(elapsed.unwrap_or_default());
    if length.is_zero() {
        length = Duration::from_secs(1);
    }
    let width = width.as_f32().max(0.);
    let scale = |at: Duration| (at.as_secs_f64() / length.as_secs_f64() * f64::from(width)) as f32;
    let mut order: Vec<usize> = (0..timeline.spans.len()).collect();
    order.sort_by_key(|&ix| timeline.spans[ix].start);
    let rows = (order.into_iter())
        .map(|ix| {
            let span = &timeline.spans[ix];
            let tone = match span.local {
                true => Tone::Local,
                false => Tone::Other,
            };
            let start = scale(span.start);
            let (mark, duration) = match span.fresh {
                true => {
                    let x = start.min(width - FRESH_WIDTH.as_f32()).max(0.);
                    (Mark::Line { x: px(x) }, None)
                }
                false => {
                    let bar = (scale(end(span)) - start).max(MIN_BAR_WIDTH.as_f32());
                    let bar = bar.min(width);
                    let x = start.min(width - bar).max(0.);
                    let mark = Mark::Bar {
                        x: px(x),
                        width: px(bar),
                    };
                    (mark, Some(end(span).saturating_sub(span.start)))
                }
            };
            CrateLayout {
                span: ix,
                mark,
                tone,
                duration,
                running: span.running && elapsed.is_some(),
            }
        })
        .collect();
    let step = tick_step(length);
    // Every `every`th tick labelled, the first among them, so labels are
    // at least LABEL_GAP apart.
    let apart = scale(step);
    let every = match apart > 0. {
        true => (LABEL_GAP.as_f32() / apart).ceil().max(1.) as u32,
        false => 1,
    };
    let ticks = (0u32..)
        .map(|n| (n, step * n))
        .take_while(|(_, at)| *at <= length)
        .map(|(n, at)| Tick {
            x: px(scale(at)),
            at,
            labelled: n % every == 0,
        })
        .collect();
    Layout {
        length,
        step,
        ticks,
        rows,
    }
}

/// How many warnings and errors each of `timeline`'s crates has, by its
/// index in the spans: each message counted for the crate it's of
/// ([`message_crate`]), if any, a relative file in one of `dirs`.
pub(crate) fn crate_counts(timeline: &BuildTimeline, dirs: &[PathBuf]) -> Vec<(usize, usize)> {
    let mut counts = vec![(0, 0); timeline.spans.len()];
    for message in &timeline.messages {
        let Some(ix) = message_crate(message, &timeline.spans, dirs) else {
            continue;
        };
        match message.level.starts_with("error") {
            true => counts[ix].1 += 1,
            false => counts[ix].0 += 1,
        }
    }
    counts
}

/// How long a crate took, as its row says it: whole milliseconds under a
/// second, else seconds to a tenth.
pub(crate) fn duration_label(duration: Duration) -> SharedString {
    let secs = duration.as_secs_f64();
    if secs < 1. {
        let ms = (secs * 1000.).round() as u64;
        tr!("workbench-build-runs-duration-ms", ms = ms.min(999))
    } else {
        tr!(
            "workbench-build-runs-duration-secs",
            secs = (secs * 10.).round() / 10.
        )
    }
}

/// A tick's label: whole seconds, then minutes and seconds.
pub(crate) fn tick_label(at: Duration) -> SharedString {
    let secs = at.as_secs();
    tr!(
        "workbench-build-runs-tick",
        minutes = secs / 60,
        seconds = secs % 60
    )
}

/// One crate's row, as the list draws it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CrateRow {
    pub(crate) layout: CrateLayout,
    pub(crate) name: SharedString,
    /// How long it took, at the right: none if it was fresh.
    pub(crate) duration: Option<SharedString>,
    pub(crate) warnings: usize,
    pub(crate) errors: usize,
    /// What a screen reader names it.
    pub(crate) a11y: SharedString,
}

impl CrateRow {
    fn of(
        layout: CrateLayout,
        timeline: &BuildTimeline,
        (warnings, errors): (usize, usize),
    ) -> Self {
        let span = &timeline.spans[layout.span];
        let name: SharedString = span.name.clone().into();
        let duration = layout.duration.map(duration_label);
        let state = match (&duration, layout.running) {
            (_, true) => tr!("workbench-build-runs-crate-building"),
            (Some(duration), false) => {
                tr!("workbench-build-runs-crate-built", duration = duration)
            }
            (None, false) => tr!("workbench-build-runs-crate-fresh"),
        };
        let local = match layout.tone {
            Tone::Local => verbatim!("yes"),
            Tone::Other => verbatim!("no"),
        };
        let a11y = tr!(
            "workbench-build-runs-crate-row",
            name = &name,
            state = state,
            local = local,
            warnings = warnings,
            errors = errors
        );
        Self {
            layout,
            name,
            duration,
            warnings,
            errors,
            a11y,
        }
    }
}

/// What a click on a crate's row, or Enter, does: shows its messages in
/// the Problems panel, once the event that asked is over.
type Open = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// The waterfall's rows.
pub(crate) struct CrateRows {
    rows: Vec<CrateRow>,
    /// Whether the rows have a counts column: the build has messages.
    counts: bool,
    open: Open,
    /// Each drawn row's mark and its colour, by row, for tests.
    #[cfg(test)]
    pub(crate) drawn_marks: std::collections::BTreeMap<usize, (Mark, Hsla)>,
}

impl CrateRows {
    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[CrateRow] {
        &self.rows
    }
}

/// A count beside a crate: its glyph in its colour and the number.
fn count_mark(icon: Icon, colour: Hsla, n: usize) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(svg().path(icon.path()).size(px(10.)).text_color(colour))
        .child(tr!("workbench-build-runs-count", count = n))
}

impl ListDelegate for CrateRows {
    fn count(&self) -> usize {
        self.rows.len()
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let row = &self.rows[ix];
        let (mark, colour) = (row.layout.mark, row.layout.tone.colour(&c));
        #[cfg(test)]
        self.drawn_marks.insert(ix, (mark, colour));
        let row = &self.rows[ix];
        let track = div().flex_1().min_w_0().h_full().relative().child(
            div()
                .debug_selector(move || mark_selector(ix))
                .absolute()
                .left(mark.x())
                .top(MARK_TOP)
                .w(mark.width())
                .h(MARK_HEIGHT)
                .when(matches!(mark, Mark::Bar { .. }), |bar| bar.rounded(px(1.)))
                .bg(colour),
        );
        let counts = div()
            .flex_none()
            .w(COUNTS_WIDTH)
            .flex()
            .items_center()
            .gap_1()
            .text_size(UI_SMALL.size)
            .line_height(UI_SMALL.line_height)
            .text_color(c.text.muted)
            .when(row.warnings > 0, |counts| {
                counts.child(count_mark(Icon::Triangle, c.status.warning, row.warnings))
            })
            .when(row.errors > 0, |counts| {
                counts.child(count_mark(Icon::X, c.status.error, row.errors))
            });
        let body = ListRow::new(state)
            .h(ROW_HEIGHT)
            .gap_2()
            .px_3()
            .child(
                div()
                    .flex_none()
                    .w(NAME_WIDTH)
                    .truncate()
                    .font_family(code)
                    .text_size(CODE_SMALL.size)
                    .line_height(CODE_SMALL.line_height)
                    .text_color(c.text.primary)
                    .child(row.name.clone()),
            )
            .child(track)
            .when(self.counts, |body| body.child(counts))
            .child(
                div()
                    .flex_none()
                    .w(DURATION_WIDTH)
                    .flex()
                    .justify_end()
                    .text_size(UI_SMALL.size)
                    .line_height(UI_SMALL.line_height)
                    .text_color(c.text.muted)
                    .children(row.duration.clone()),
            );
        let (span, open) = (row.layout.span, self.open.clone());
        // The list selects the row; the click shows its messages too.
        div()
            .id(("build-run-row", ix))
            .debug_selector(move || row_selector(ix))
            .w_full()
            .on_click(move |_, window, cx| open(span, window, cx))
            .child(body)
            .into_any_element()
    }

    fn accessible_name(&self, _: &App) -> SharedString {
        tr!("workbench-build-runs-crates")
    }

    fn row_name(&self, ix: usize, _: &App) -> SharedString {
        self.rows[ix].a11y.clone()
    }
}

/// The build run shown.
#[derive(Clone, Debug)]
struct Shown {
    id: RunId,
    title: SharedString,
    status: RunStatus,
    summary: RunSummary,
    started: Instant,
}

/// A build run's detail: see the module docs.
pub(crate) struct BuildTimelineView {
    workbench: WeakEntity<Workbench>,
    shown: Option<Shown>,
    tab: Tab,
    /// The build as last read, and the folders its relative files are in.
    timeline: BuildTimeline,
    dirs: Vec<PathBuf>,
    /// Its crates' warning and error counts, by span.
    counts: Vec<(usize, usize)>,
    /// Its layout as the rows were last made.
    layout: Option<Layout>,
    rows: Entity<List<CrateRows>>,
    /// The track's width as last drawn.
    width: Rc<Cell<Pixels>>,
    /// The output, read again only while it shows.
    output: Vec<SharedString>,
    output_stale: bool,
    output_scroll: UniformListScrollHandle,
    /// To the build's program, while it's shown: its output and its
    /// timeline change as it runs.
    _process: Option<Subscription>,
    /// The next redraw of a running build.
    tick: Option<Task<()>>,
    _rows: Subscription,
}

impl BuildTimelineView {
    pub(crate) fn new(
        workbench: WeakEntity<Workbench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = cx.weak_entity();
        let open: Open = Rc::new(move |span, window, cx| {
            let view = view.clone();
            // Outside whatever update the click or key came in.
            window.defer(cx, move |window, cx| {
                view.update(cx, |view, cx| view.open(span, window, cx)).ok();
            });
        });
        let rows = CrateRows {
            rows: Vec::new(),
            counts: false,
            open,
            #[cfg(test)]
            drawn_marks: Default::default(),
        };
        let rows = cx.new(|cx| List::new(rows, cx));
        let events = cx.subscribe_in(&rows, window, |_, rows, event, window, cx| {
            if let ListEvent::Confirmed(ix) = *event {
                let delegate = rows.read(cx).delegate();
                if let Some(row) = delegate.rows.get(ix) {
                    (delegate.open.clone())(row.layout.span, window, cx);
                }
            }
        });
        Self {
            workbench,
            shown: None,
            tab: Tab::default(),
            timeline: BuildTimeline::default(),
            dirs: Vec::new(),
            counts: Vec::new(),
            layout: None,
            rows,
            width: Rc::new(Cell::new(px(0.))),
            output: Vec::new(),
            output_stale: true,
            output_scroll: UniformListScrollHandle::new(),
            _process: None,
            tick: None,
            _rows: events,
        }
    }

    /// The run shown, if any.
    pub(crate) fn run(&self) -> Option<RunId> {
        self.shown.as_ref().map(|shown| shown.id)
    }

    /// The view shown.
    #[cfg(test)]
    pub(crate) fn tab(&self) -> Tab {
        self.tab
    }

    /// The waterfall's rows.
    #[cfg(test)]
    pub(crate) fn rows(&self) -> &Entity<List<CrateRows>> {
        &self.rows
    }

    /// The layout the rows were last made with.
    #[cfg(test)]
    pub(crate) fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    /// The track's width as last drawn.
    #[cfg(test)]
    pub(crate) fn track_width(&self) -> Pixels {
        self.width.get()
    }

    /// The output, as last read.
    #[cfg(test)]
    pub(crate) fn output(&self) -> &[SharedString] {
        &self.output
    }

    /// Shows `tab`.
    pub(crate) fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.output_stale = true;
            cx.notify();
        }
    }

    /// Shows the build run `run`, as it is now, or nothing. A run shown
    /// afresh starts on the Timeline; one already shown keeps its view.
    pub(crate) fn show(&mut self, run: Option<RunId>, cx: &mut Context<Self>) {
        let workbench = self.workbench.upgrade();
        let found = run.zip(workbench.as_ref()).and_then(|(id, workbench)| {
            let workbench = workbench.read(cx);
            let run = workbench.runs().read(cx).get(id)?;
            if run.kind != RunKind::Build {
                return None;
            }
            Some(Shown {
                id,
                title: run.title.clone().into(),
                status: run.status,
                summary: run.summary,
                started: run.started,
            })
        });
        let Some(shown) = found else {
            if self.shown.take().is_some() {
                self._process = None;
                self.tick = None;
                self.tab = Tab::default();
                self.read(cx);
            }
            return;
        };
        if self.run() != Some(shown.id) {
            self.tab = Tab::default();
            let process = (workbench.as_ref())
                .and_then(|workbench| workbench.read(cx).build_process(shown.id, cx));
            self._process =
                process.map(|process| cx.observe(&process, |view, _, cx| view.read(cx)));
            self.output_scroll = UniformListScrollHandle::new();
        }
        self.shown = Some(shown);
        self.read(cx);
    }

    /// Reads the build shown again and makes its rows.
    fn read(&mut self, cx: &mut Context<Self>) {
        let workbench = self.workbench.upgrade();
        let read = (self.shown.as_ref())
            .zip(workbench)
            .and_then(|(shown, workbench)| workbench.read(cx).build_shown(shown.id, cx));
        (self.timeline, self.dirs) = read.unwrap_or_default();
        self.counts = crate_counts(&self.timeline, &self.dirs);
        self.output_stale = true;
        self.rebuild(cx);
    }

    /// How long the build shown has been running, if it is.
    fn elapsed(&self) -> Option<Duration> {
        let shown = self.shown.as_ref()?;
        shown.status.is_running().then(|| shown.started.elapsed())
    }

    /// Lays the waterfall out again at the track's width and the time, and
    /// makes the rows; while the build runs, does it again in a
    /// [`LIVE_TICK`].
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let elapsed = self.elapsed();
        let layout = layout(&self.timeline, elapsed, self.width.get());
        let rows: Vec<CrateRow> = (layout.rows.iter().cloned())
            .map(|row| {
                let counts = self.counts.get(row.span).copied().unwrap_or_default();
                CrateRow::of(row, &self.timeline, counts)
            })
            .collect();
        let has_messages = !self.timeline.messages.is_empty();
        self.rows.update(cx, |list, cx| {
            let delegate = list.delegate_mut();
            delegate.rows = rows;
            delegate.counts = has_messages;
            list.rows_changed(cx);
        });
        self.layout = Some(layout);
        self.tick = elapsed.map(|_| {
            cx.spawn(async move |view, cx| {
                cx.background_executor().timer(LIVE_TICK).await;
                view.update(cx, |view, cx| view.rebuild(cx)).ok();
            })
        });
        cx.notify();
    }

    /// The track's width as just drawn: the rows again if it's changed.
    fn set_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if self.width.replace(width) != width {
            self.rebuild(cx);
        }
    }

    /// Shows the Problems panel with the crate `span`'s first message
    /// selected, if the panel lists one.
    fn open(&mut self, span: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(run), Some(workbench)) = (self.run(), self.workbench.upgrade()) else {
            return;
        };
        workbench.update(cx, |workbench, cx| {
            workbench.show_crate_problems(run, span, window, cx)
        });
    }

    /// The output again, if it's changed since it was read.
    fn read_output(&mut self, cx: &App) {
        if !std::mem::take(&mut self.output_stale) {
            return;
        }
        let workbench = self.workbench.upgrade();
        let lines = (self.run().zip(workbench))
            .and_then(|(run, workbench)| workbench.read(cx).build_output(run, cx));
        self.output = (lines.unwrap_or_default().into_iter())
            .map(SharedString::from)
            .collect();
    }

    /// The axis along the top of the waterfall: the ticks and their labels
    /// over the track, and the build's length over the durations. It
    /// measures the track.
    fn axis(&self, layout: &Layout, counts: bool, cx: &mut Context<Self>) -> gpui::Div {
        let c = cx.theme().colors;
        let (cell, view) = (self.width.clone(), cx.weak_entity());
        let measure = canvas(
            move |bounds, _, cx| {
                let width = bounds.size.width;
                if cell.get() != width {
                    view.update(cx, |view, cx| view.set_width(width, cx)).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let ticks = layout.ticks.iter().map(|tick| {
            let label = (tick.labelled).then(|| {
                div()
                    .absolute()
                    .left(px(3.))
                    .top_0()
                    .whitespace_nowrap()
                    .child(tick_label(tick.at))
            });
            div()
                .absolute()
                .left(tick.x)
                .top_0()
                .h_full()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .w(px(1.))
                        .h(TICK_HEIGHT)
                        .bg(c.border.subtle),
                )
                .children(label)
        });
        div()
            .debug_selector(|| AXIS_SELECTOR.into())
            .flex_none()
            .h(ROW_HEIGHT)
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(c.border.subtle)
            .text_size(UI_SMALL.size)
            .line_height(UI_SMALL.line_height)
            .text_color(c.text.muted)
            .child(div().flex_none().w(NAME_WIDTH))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .overflow_hidden()
                    .child(measure)
                    .children(ticks),
            )
            .when(counts, |axis| axis.child(div().flex_none().w(COUNTS_WIDTH)))
            .child(
                div()
                    .flex_none()
                    .w(DURATION_WIDTH)
                    .flex()
                    .justify_end()
                    .child(duration_label(layout.length)),
            )
    }

    fn timeline(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = cx.theme().colors;
        let Some(layout) = self.layout.clone() else {
            return div().into_any_element();
        };
        if layout.rows.is_empty() {
            let empty = match self.elapsed() {
                Some(_) => tr!("workbench-build-runs-no-crates-yet"),
                None => tr!("workbench-build-runs-no-crates"),
            };
            return div()
                .px_3()
                .py_1()
                .text_size(UI_SMALL.size)
                .text_color(c.text.muted)
                .child(empty)
                .into_any_element();
        }
        let counts = !self.timeline.messages.is_empty();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.axis(&layout, counts, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(self.rows.clone()),
            )
            .into_any_element()
    }

    fn output_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.read_output(cx);
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        if self.output.is_empty() {
            return div()
                .px_3()
                .py_1()
                .text_size(UI_SMALL.size)
                .text_color(c.text.muted)
                .child(tr!("workbench-build-runs-no-output"))
                .into_any_element();
        }
        let lines = self.output.clone();
        div()
            .debug_selector(|| OUTPUT_SELECTOR.into())
            .flex_1()
            .min_h_0()
            .px_3()
            .font_family(code)
            .text_size(CODE_SMALL.size)
            .line_height(CODE_SMALL.line_height)
            .text_color(c.text.primary)
            .child(
                uniform_list("build-run-output-lines", lines.len(), move |range, _, _| {
                    (lines[range].iter())
                        .map(|line| {
                            div()
                                .h(CODE_SMALL.line_height)
                                .whitespace_nowrap()
                                .child(line.clone())
                                .into_any_element()
                        })
                        .collect()
                })
                .track_scroll(&self.output_scroll)
                .size_full(),
            )
            .into_any_element()
    }
}

impl Render for BuildTimelineView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let Some(shown) = self.shown.clone() else {
            return div().into_any_element();
        };
        let view = cx.weak_entity();
        let switch = SegmentedControl::new(SWITCH_ID, tr!("workbench-build-runs-view"))
            .small()
            .segments(Tab::ALL.map(Tab::label))
            .selected(
                Tab::ALL
                    .iter()
                    .position(|tab| *tab == self.tab)
                    .unwrap_or(0),
            )
            .on_change(move |ix: &usize, _, cx| {
                let tab = Tab::ALL.get(*ix).copied().unwrap_or_default();
                view.update(cx, |view, cx| view.set_tab(tab, cx)).ok();
            });
        let body = match self.tab {
            Tab::Timeline => self.timeline(cx),
            Tab::Output => self.output_view(cx),
        };
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(fe_ui::ROW_HEIGHT + px(4.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink(1.)
                            .min_w_0()
                            .truncate()
                            .font_family(code)
                            .text_size(CODE_SMALL.size)
                            .line_height(CODE_SMALL.line_height)
                            .text_color(c.text.primary)
                            .child(shown.title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(UI_SMALL.size)
                            .line_height(UI_SMALL.line_height)
                            .text_color(c.text.muted)
                            .child(summary_text(shown.status, shown.summary)),
                    )
                    .child(div().flex_none().child(switch)),
            )
            .child(body)
            .into_any_element()
    }
}
